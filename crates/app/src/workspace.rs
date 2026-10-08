//! The window's root view: owns [`AppState`], drives every async device call
//! through `background_spawn` with sequence guards, and renders the §6 shell
//! (top bar, folder tree, browser/dashboard column, inspector, overlays).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use android18_core::domain::{
    ColorTag, Device, SortDirection, SortField, SortSpec, TransferDirection, ViewMode,
};
use android18_core::fs::paths::{STORAGE_ROOT, display_path, is_descendant, parent_of};
use android18_core::port::{DeviceBackend, SharedBackend, SharedSearch};
use android18_core::search::HeuristicSearchProvider;
use android18_core::shell::{OutputKind, ShellLine, ShellSession};
use android18_core::transfer::{TransferEvent, apply, new_transfer};
use android18_core::util::categorize::FileCategory;
use android18_transport::pairing::{
    PairGrant, PairRequestError, PairingEvent, PairingListener, PairingPayload, lan_ip,
};
use android18_transport::{DiscoverySource, HttpDevice, HttpSearchProvider};
use gpui_kit::WindowBounds;
use gpui_kit::base::input::InputEditorStyle;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement, PathPromptOptions, RenderImage,
    ScrollHandle, Styled, Subscription, Task, Window, div, px,
};

use crate::download::{self, DownloadCommand, DownloadJob};
use crate::settings::{AppSettings, DefaultView, WindowState, settings_path};
use crate::state::{
    AppState, CenterView, Clipboard, ClipboardMode, DeviceCandidate, DeviceSource, QuickFilter,
    Toast,
};
use crate::thumb;
use crate::ui;
use crate::upload::{self, UploadJob};

gpui_kit::actions!(
    workspace,
    [
        NavigateBack,
        NavigateForward,
        NavigateUp,
        Refresh,
        ToggleViewMode,
        CycleSort,
        OpenSelected,
        CopySelectedPath,
        CopyCurrentPath,
        DownloadSelected,
        DeleteSelected,
        RenameSelected,
        CopySelected,
        CutSelected,
        PasteIntoFolder,
        TogglePin,
        TagBlue,
        TagEmerald,
        TagAmber,
        TagPurple,
        TagRose,
        TagSlate,
        ClearTag,
        NewFolder,
        UploadFiles,
        ToggleSearch,
        ToggleTerminal,
        TerminalHistoryPrev,
        TerminalHistoryNext,
        ToggleTransfers,
        ToggleServer,
        ToggleInspector,
        ToggleDashboard,
        Escape,
        About,
        Settings,
        Minimize,
        Zoom,
        ToggleFullscreen,
        Quit
    ]
);

pub struct Workspace {
    pub state: AppState,
    pub search_input: Entity<InputState>,
    pub terminal_input: Entity<InputState>,
    pub new_folder_input: Entity<InputState>,
    /// §10 dashboard recents instant-search box.
    pub recent_search_input: Entity<InputState>,
    /// Onboarding-gate "Advanced" manual address + optional 6-char pair code.
    pub device_url_input: Entity<InputState>,
    pub manual_code_input: Entity<InputState>,
    /// Inline pair-code entry under the discovered row being connected.
    pub device_code_input: Entity<InputState>,
    /// R4 rename modal input.
    pub rename_input: Entity<InputState>,
    /// Keyboard focus owned by the workspace root div. Menu-item validation
    /// and keymap dispatch both ride the focus path, so whenever no modal
    /// input holds focus the root must — otherwise every `on_action` below
    /// is unreachable and the whole menu bar validates disabled (B12).
    root_focus: FocusHandle,
    /// Deferred single-click inspector open; a newer click replaces it.
    pending_inspect: Option<Task<()>>,
    /// Debounced window-frame save (dropping cancels the timer).
    pending_window_save: Option<Task<()>>,
    /// Last Dock badge count pushed to AppKit.
    dock_badge: usize,
    /// Position in the shell history while recalling lines with ↑/↓.
    terminal_history_cursor: Option<usize>,
    /// Persisted app settings (Settings modal; loaded at launch).
    pub settings: AppSettings,
    /// First click of "Forget this device" arms; the second confirms.
    pub forget_armed: bool,
    /// `None` while a command runs on the background executor.
    shell: Mutex<Option<ShellSession>>,
    /// Live QR-pairing listener while the onboarding gate is up (offline).
    pairing: Option<PairingListener>,
    /// Control channels to running download jobs (transfer id → sender).
    download_controls: HashMap<u64, mpsc::Sender<DownloadCommand>>,
    /// Control channels to running upload jobs (transfer id → sender).
    upload_controls: HashMap<u64, mpsc::Sender<upload::UploadCommand>>,
    /// Grid-tile thumbnails keyed by remote path (live sessions only).
    pub thumbnails: HashMap<String, Arc<RenderImage>>,
    /// Paths with an in-flight thumbnail request.
    pending_thumbs: HashSet<String>,
    /// Paths whose thumbnail fetch/decode failed; not retried until reconnect.
    failed_thumbs: HashSet<String>,
    /// Insertion order of `thumbnails`, oldest first (FIFO eviction).
    thumb_order: VecDeque<String>,
    /// Paths the visible rows want, nearest first; drained by `pump_thumbs`.
    thumb_queue: VecDeque<String>,
    /// A coalesced re-render is already scheduled for arrived thumbnails.
    thumb_notify_armed: bool,
    /// Scroll state of the list/grid, used to fetch only visible thumbnails.
    pub table_scroll: ScrollHandle,
    pub grid_scroll: ScrollHandle,
    subscriptions: Vec<Subscription>,
}

/// Concurrent thumbnail requests and decoded-thumbnail cache bound.
const MAX_THUMB_INFLIGHT: usize = 6;
/// Head of a text file fetched for the inspector preview.
const TEXT_PREVIEW_BYTES: u64 = 32 * 1024;

/// Files worth a text preview: everything except formats known to be binary.
fn text_previewable(entry: &android18_core::domain::Entry) -> bool {
    if matches!(
        FileCategory::of(entry),
        FileCategory::Audio
            | FileCategory::Archive
            | FileCategory::AndroidPackage
            | FileCategory::BinaryImage
    ) {
        return false;
    }
    !entry.extension.as_deref().is_some_and(|e| {
        ["doc", "docx", "xls", "xlsx", "rtf"]
            .iter()
            .any(|b| e.eq_ignore_ascii_case(b))
    })
}
const MAX_THUMB_CACHE: usize = 1500;

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Ask AI: camera photos, large files…")
        });
        let terminal_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("Type a command — try `help`");
            // The terminal panel is dark slate (§11) and the kit input
            // re-projects light-theme ink on every render, so the editor
            // style is fixed here, once, and never re-projected.
            state.set_editor_style(InputEditorStyle {
                foreground: ui::theme::SLATE_200,
                muted_foreground: ui::theme::SLATE_500,
                background: ui::theme::SLATE_950,
                border: ui::theme::SLATE_800,
                selection: ui::theme::EMERALD_500.opacity(0.4),
                caret: ui::theme::EMERALD_500,
                ..InputEditorStyle::default()
            });
            state
        });
        let new_folder_input = cx.new(|cx| InputState::new(window, cx).placeholder("folder name"));
        let recent_search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search recent activity…"));
        let device_url_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("http://192.168.1.42:8080"));
        let manual_code_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Pair code (optional, 6 characters)")
        });
        let device_code_input = cx.new(|cx| InputState::new(window, cx).placeholder("0X1D8C"));
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("new name"));
        let root_focus = cx.focus_handle();

        let mut this = Self {
            state: AppState::new(ui::NOW_MS),
            search_input,
            terminal_input,
            new_folder_input,
            recent_search_input,
            device_url_input,
            manual_code_input,
            device_code_input,
            rename_input,
            root_focus,
            settings: AppSettings::load(),
            forget_armed: false,
            shell: Mutex::new(Some(ShellSession::new("phone"))),
            pairing: None,
            download_controls: HashMap::new(),
            upload_controls: HashMap::new(),
            thumbnails: HashMap::new(),
            pending_thumbs: HashSet::new(),
            failed_thumbs: HashSet::new(),
            thumb_order: VecDeque::new(),
            thumb_queue: VecDeque::new(),
            thumb_notify_armed: false,
            table_scroll: ScrollHandle::new(),
            grid_scroll: ScrollHandle::new(),
            pending_inspect: None,
            pending_window_save: None,
            dock_badge: 0,
            terminal_history_cursor: None,
            subscriptions: Vec::new(),
        };
        // Focus the root from the first frame so the action dispatch path —
        // menus and the keymap — exists before any interaction (B12).
        window.focus(&this.root_focus, cx);
        // Persisted default view (Settings modal) applies at launch.
        this.state.view_mode = this.settings.default_view.into();
        // R3 launch behavior: silently reconnect when a phone was paired
        // before (offline placeholder until it answers). Either way the
        // onboarding gate is the first screen — pairing runs on it.
        let last = android18_transport::load_last_device()
            .filter(|device| android18_transport::load_token(&device.id).is_some());
        let first_run = last.is_none();
        if let Some(device) = last {
            this.state.device_info = device;
            this.state.device = AppState::empty_backend(ui::NOW_MS);
        }
        // Welcome banner from the live shell session.
        if let Some(session) = this.shell.lock().expect("shell mutex").as_ref() {
            this.state.transcript = session.welcome(&this.state.device_info);
        }

        // Enter in the terminal input runs the line; Enter in search runs the query.
        let terminal_sub = cx.subscribe_in(
            &this.terminal_input,
            window,
            |this, _input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_terminal(window, cx);
                }
            },
        );
        let search_sub = cx.subscribe(
            &this.search_input,
            |this, _input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.run_search(cx);
                }
            },
        );
        // Typing the sixth character of the pair code submits it.
        let code_sub = cx.subscribe(
            &this.device_code_input,
            |this, _input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                    this.submit_code(cx);
                }
            },
        );
        let manual_sub = cx.subscribe(
            &this.manual_code_input,
            |this, _input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.connect_manual(cx);
                }
            },
        );
        let bounds_sub = cx.observe_window_bounds(window, |this, window, cx| {
            this.schedule_window_save(window, cx);
        });
        this.subscriptions
            .extend([terminal_sub, search_sub, code_sub, manual_sub, bounds_sub]);

        // No device I/O before a backend lands: the offline placeholder
        // behind the gate answers like a real phone (mock 401s), so eager
        // launch refreshes would raise bogus errors. Live data loads via
        // `apply_live_backend` once a phone answers.
        // R3: silently reconnect to the last paired phone, if any.
        this.try_reconnect(cx);
        // The onboarding gate is up in both cases, so it needs a QR
        // session and the device watch. First run starts immediately;
        // with a remembered phone the watch waits out the silent
        // reconnect and says so.
        if first_run {
            this.start_pairing_session(cx);
            this.start_device_watch(Duration::ZERO, cx);
        } else {
            this.state.device_scan = format!("Reconnecting to {}…", this.state.device_info.name);
            this.start_pairing_session(cx);
            this.start_device_watch(Duration::from_secs(5), cx);
        }
        this
    }

    /// Current prompt, or `None` while a command runs.
    pub fn shell_prompt(&self) -> Option<String> {
        self.shell
            .lock()
            .expect("shell mutex")
            .as_ref()
            .map(ShellSession::prompt)
    }

    /// Fetches the current directory listing in the background; a sequence
    /// guard drops responses that arrive after a newer navigation. No-op
    /// while offline: the onboarding gate owns the window and the
    /// placeholder backend never serves listings.
    pub fn refresh_list(&mut self, cx: &mut Context<Self>) {
        if !self.state.live {
            return;
        }
        self.state.list_seq += 1;
        let seq = self.state.list_seq;
        let device = self.state.device.clone();
        let cwd = self.state.cwd.clone();
        let token = self.state.token();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { device.list(&cwd, &token).await })
                .await;
            _ = this.update(cx, |ws, cx| {
                if ws.state.list_seq != seq {
                    return;
                }
                match result {
                    Ok(entries) => {
                        ws.state.entries = entries;
                        ws.state.error = None;
                    }
                    Err(e) => {
                        // Never leave the previous folder's rows under a
                        // path that failed to load.
                        ws.state.entries.clear();
                        ws.state.error = Some(e.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Dashboard refresh: reloads the listing and re-measures the whole
    /// volume (used/total bytes, tree snapshot).
    pub fn refresh_dashboard(&mut self, cx: &mut Context<Self>) {
        self.refresh_list(cx);
        self.refresh_storage(cx);
    }

    /// Walks the whole tree once: storage totals, sidebar tree, dashboard.
    /// No-op while offline, like `refresh_list`.
    fn refresh_storage(&mut self, cx: &mut Context<Self>) {
        if !self.state.live {
            return;
        }
        self.state.storage_seq += 1;
        let seq = self.state.storage_seq;
        let device = self.state.device.clone();
        let token = self.state.token();
        cx.spawn(async move |this, cx| {
            let (snapshot, info) = cx
                .background_executor()
                .spawn(async move {
                    let info = device.device_info().await.ok();
                    (device.walk(STORAGE_ROOT, &token).await.ok(), info)
                })
                .await;
            _ = this.update(cx, |ws, cx| {
                if ws.state.storage_seq != seq {
                    return;
                }
                if let Some(info) = info {
                    ws.state.device_info.storage_used_bytes = info.storage_used_bytes;
                    ws.state.device_info.storage_total_bytes = info.storage_total_bytes;
                }
                if let Some(all) = snapshot {
                    let files: u64 = all.iter().filter(|e| !e.dir).map(|e| e.size).sum();
                    ws.state.storage_files_bytes = files;
                    // The phone's figure covers apps/system too; never show
                    // less than the files we can see.
                    ws.state.storage_used = ws.state.device_info.storage_used_bytes.max(files);
                    ws.state.all_folders = all.iter().filter(|e| e.dir).cloned().collect();
                    ws.state.all_entries = all;
                }
                cx.notify();
            });
        })
        .detach();
    }
    /// Navigates to `path`, recording it on the back/forward history.
    pub fn navigate(&mut self, path: String, cx: &mut Context<Self>) {
        if path == self.state.cwd && self.state.quick_filter.is_none() {
            // Already there, but the dashboard may be on screen (category
            // cards, recents): still reveal the browser.
            if self.state.view != CenterView::Browser {
                self.state.view = CenterView::Browser;
                cx.notify();
            }
            return;
        }
        self.state.cwd = path.clone();
        self.state.push_history(&path);
        self.state.selected = None;
        self.state.selected_set.clear();
        self.state.selected_preview = None;
        self.state.selected_preview_image = None;
        self.state.hovered_row = None;
        self.state.quick_filter = None;
        self.state.inspector_open = false;
        self.state.view = CenterView::Browser;
        self.state.error = None;
        self.refresh_list(cx);
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if !self.state.can_go_back() {
            return;
        }
        self.state.history_index -= 1;
        let path = self.state.history[self.state.history_index].clone();
        self.navigate_historyless(path);
        self.refresh_list(cx);
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        if !self.state.can_go_forward() {
            return;
        }
        self.state.history_index += 1;
        let path = self.state.history[self.state.history_index].clone();
        self.navigate_historyless(path);
        self.refresh_list(cx);
    }

    fn navigate_historyless(&mut self, path: String) {
        self.state.cwd = path;
        self.state.selected = None;
        self.state.selected_set.clear();
        self.state.selected_preview = None;
        self.state.selected_preview_image = None;
        self.state.hovered_row = None;
        self.state.quick_filter = None;
        self.state.inspector_open = false;
        self.state.view = CenterView::Browser;
        self.state.error = None;
    }

    pub fn navigate_up(&mut self, cx: &mut Context<Self>) {
        if let Some(parent) = parent_of(&self.state.cwd) {
            self.navigate(parent, cx);
        }
    }

    /// Double-click / tree activation: folders navigate, files select.
    /// A double-click is not an inspect — files select silently, so the
    /// inspector never pops open under an activation click.
    pub fn open_entry(&mut self, path: String, dir: bool, cx: &mut Context<Self>) {
        if dir {
            self.navigate(path, cx);
        } else {
            self.select_only(path, cx);
        }
    }

    /// Single click: the selection (and row highlight) lands immediately;
    /// the inspector opens on a ~250 ms defer so the first click of a
    /// double-click never flashes it open. Each new click replaces (and so
    /// cancels) the pending open; navigation clears the anchor, which the
    /// guard inside the task re-checks.
    pub fn select_entry(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        match path {
            None => {
                self.cancel_pending_inspect();
                self.state.selected = None;
                self.state.selected_preview = None;
                self.state.selected_preview_image = None;
            }
            Some(path) => {
                self.state.selected = Some(path.clone());
                self.state.selected_preview = None;
                self.state.selected_preview_image = None;
                self.pending_inspect = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                    _ = this.update(cx, |ws, cx| {
                        if ws.state.selected.as_deref() != Some(path.as_str()) {
                            return; // superseded by a later click
                        }
                        ws.state.inspector_open = true;
                        ws.fetch_preview(path, cx);
                        cx.notify();
                    });
                }));
            }
        }
        cx.notify();
    }

    /// Kills a pending deferred inspector open (close button, Escape).
    fn cancel_pending_inspect(&mut self) {
        self.pending_inspect = None; // dropping the task cancels its timer
    }

    /// Selection without side effects: updates the anchor (keeping a
    /// multi-select set the entry belongs to) but never opens the
    /// inspector. Used by right-clicks and row hover actions.
    pub fn select_only(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.state.is_selected(&path) {
            self.state.selected_set.clear();
            self.state.selected = Some(path.clone());
            self.state.selected_preview = None;
            self.state.selected_preview_image = None;
            // The panel is already open on another entry: refresh its
            // preview quietly instead of popping it open.
            if self.state.inspector_open {
                self.fetch_preview(path, cx);
            }
        }
        cx.notify();
    }

    /// R4 row click with modifiers: ⌘ toggles membership in the
    /// multi-select set, ⇧ selects the range from the anchor, double-click
    /// activates, plain clicks single-select.
    pub fn row_clicked(
        &mut self,
        path: String,
        dir: bool,
        command: bool,
        shift: bool,
        double: bool,
        cx: &mut Context<Self>,
    ) {
        if command {
            let anchor = self.state.selected.clone();
            let mut set = self.state.selected_set.clone();
            if set.is_empty()
                && let Some(anchor) = anchor.as_deref()
            {
                set.insert(anchor.to_string());
            }
            if set.contains(&path) {
                set.remove(&path);
            } else {
                set.insert(path.clone());
            }
            self.state.selected_set = set;
            self.state.selected = Some(path);
        } else if shift {
            let Some(anchor) = self.state.selected.clone() else {
                self.row_clicked(path, dir, false, false, false, cx);
                return;
            };
            let visible: Vec<String> = self
                .state
                .sorted_entries()
                .iter()
                .map(|entry| entry.path.clone())
                .collect();
            let anchor_index = visible.iter().position(|p| p == &anchor);
            let target_index = visible.iter().position(|p| p == &path);
            if let (Some(from), Some(to)) = (anchor_index, target_index) {
                let range = from.min(to)..=from.max(to);
                self.state.selected_set = visible[range].iter().cloned().collect();
                self.state.selected_set.remove(&anchor);
                self.state.selected_set.insert(anchor.clone());
                self.state.selected = Some(path);
            }
        } else if double {
            self.open_entry(path, dir, cx);
            return;
        } else {
            self.state.selected_set.clear();
            self.select_entry(Some(path), cx);
            return;
        }
        cx.notify();
    }

    /// Right-click selection rule: a member of the multi-select keeps the
    /// set intact; anything else restarts selection at that row. The
    /// selection is silent — no inspector, no preview fetch — so the
    /// context menu never moves the UI underneath it.
    pub fn right_click_entry(&mut self, path: String, cx: &mut Context<Self>) {
        self.select_only(path, cx);
    }

    /// Loads a preview for the selection in the background, by category:
    /// images/videos on a live device fetch a full-size `/thumb` poster
    /// (decoded off-thread); everything else tries inline content first,
    /// then `read_text`. Binary markers come back as `None` so the
    /// inspector shows a proper fallback instead of "[Binary File]".
    fn fetch_preview(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(entry) = self.state.entry_at(&path).cloned() else {
            return;
        };
        if entry.dir {
            return; // folders carry no preview
        }
        // Images, video frames and PDF first pages all come from `/thumb`.
        let media = ui::browser::thumbable(&entry);
        self.state.list_seq += 1; // reuse the listing guard for previews
        let seq = self.state.list_seq;
        let device = self.state.device.clone();
        let token = self.state.token();
        if media && self.state.live {
            cx.spawn(async move |this, cx| {
                let fetch_path = path.clone();
                let bytes = cx
                    .background_executor()
                    .spawn(async move {
                        device
                            .thumb(&fetch_path, thumb::PREVIEW_MAX_DIM, &token)
                            .await
                    })
                    .await;
                _ = this.update(cx, |ws, cx| {
                    if ws.state.list_seq != seq {
                        return;
                    }
                    if ws.state.selected.as_deref() == Some(path.as_str())
                        && let Some(rendered) = bytes.as_deref().ok().and_then(thumb::decode)
                    {
                        ws.state.selected_preview_image = Some(Arc::new(rendered));
                        cx.notify();
                    }
                });
            })
            .detach();
            return;
        }
        if entry.content.is_some() {
            return; // inline text — nothing to fetch
        }
        if media {
            return; // no live device → no poster; placeholder renders
        }
        if !text_previewable(&entry) {
            return; // opaque payload — the fallback panel renders
        }
        cx.spawn(async move |this, cx| {
            let fetch_path = path.clone();
            // Only the head of the file: whole-file reads of big payloads
            // would stall the panel for a few lines of preview.
            let result = cx
                .background_executor()
                .spawn(async move {
                    device
                        .read_range(&fetch_path, 0, TEXT_PREVIEW_BYTES, &token)
                        .await
                        .map(|bytes| {
                            if bytes.contains(&0) {
                                "[Binary File]".to_string()
                            } else {
                                String::from_utf8_lossy(&bytes).into_owned()
                            }
                        })
                })
                .await;
            _ = this.update(cx, |ws, cx| {
                if ws.state.list_seq != seq {
                    return;
                }
                if ws.state.selected.as_deref() == Some(path.as_str()) {
                    ws.state.selected_preview = match result {
                        // The mock's placeholder for opaque payloads is not
                        // a preview — fall through to the fallback panel.
                        Ok(text) if text.starts_with("[Binary File") => None,
                        Ok(text) => Some(text),
                        Err(_) => None,
                    };
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub fn toggle_view_mode(&mut self, cx: &mut Context<Self>) {
        self.state.view_mode = match self.state.view_mode {
            ViewMode::Table => ViewMode::Grid,
            ViewMode::Grid => ViewMode::Table,
        };
        cx.notify();
    }

    pub fn cycle_sort(&mut self, cx: &mut Context<Self>) {
        self.state.sort = match self.state.sort.field {
            SortField::Name => SortSpec {
                field: SortField::Size,
                direction: SortDirection::Asc,
            },
            SortField::Size => SortSpec {
                field: SortField::Mtime,
                direction: SortDirection::Desc,
            },
            SortField::Mtime => SortSpec {
                field: SortField::Type,
                direction: SortDirection::Asc,
            },
            SortField::Type => SortSpec {
                field: SortField::Name,
                direction: SortDirection::Asc,
            },
        };
        cx.notify();
    }

    pub fn toggle_center_view(&mut self, cx: &mut Context<Self>) {
        self.state.view = match self.state.view {
            CenterView::Browser => CenterView::Dashboard,
            CenterView::Dashboard => CenterView::Browser,
        };
        self.state.selected = None;
        self.state.inspector_open = false;
        cx.notify();
    }

    /// Dashboard "Browse Storage": lands on the storage root itself.
    pub fn browse_storage(&mut self, cx: &mut Context<Self>) {
        self.navigate(STORAGE_ROOT.to_string(), cx);
    }

    /// §8.2 quick access: toggles a file-type/tag filter. The browser
    /// becomes a virtual folder over the tree snapshot; clicking the same
    /// row again (or navigating anywhere) clears it.
    pub fn open_quick_filter(&mut self, filter: QuickFilter, cx: &mut Context<Self>) {
        self.state.quick_filter = if self.state.quick_filter == Some(filter.clone()) {
            None
        } else {
            Some(filter)
        };
        self.state.clear_selection();
        self.state.selected_preview = None;
        self.state.selected_preview_image = None;
        self.state.hovered_row = None;
        self.state.inspector_open = false;
        self.state.view = CenterView::Browser;
        self.state.error = None;
        cx.notify();
    }

    pub fn toggle_tree_node(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.state.expanded.insert(path.to_string()) {
            self.state.expanded.remove(path);
        }
        cx.notify();
    }

    pub fn toggle_inspector(&mut self, cx: &mut Context<Self>) {
        if !self.state.live {
            return;
        }
        if self.state.selected.is_some() {
            self.state.inspector_open = !self.state.inspector_open;
        } else {
            self.state.inspector_open = false;
        }
        if !self.state.inspector_open {
            // Never let a pending single-click reopen the panel the user
            // just closed.
            self.cancel_pending_inspect();
        }
        cx.notify();
    }

    pub fn toggle_terminal(&mut self, cx: &mut Context<Self>) {
        if !self.state.live {
            return;
        }
        self.state.terminal_open = !self.state.terminal_open;
        cx.notify();
    }

    pub fn toggle_terminal_max(&mut self, cx: &mut Context<Self>) {
        self.state.terminal_maximized = !self.state.terminal_maximized;
        cx.notify();
    }

    pub fn toggle_transfers(&mut self, cx: &mut Context<Self>) {
        if !self.state.live {
            return;
        }
        self.state.transfers_open = !self.state.transfers_open;
        cx.notify();
    }

    /// Opens/closes the server sheet — the connected-phone surface. It is
    /// a no-op while offline: pairing lives on the onboarding gate, which
    /// is what the user sees instead.
    pub fn toggle_server_sheet(&mut self, cx: &mut Context<Self>) {
        if !self.state.live {
            return;
        }
        self.state.server_open = !self.state.server_open;
        cx.notify();
    }

    /// Closes the server sheet.
    pub fn close_server_sheet(&mut self, cx: &mut Context<Self>) {
        self.state.server_open = false;
        cx.notify();
    }

    /// Retires the QR listener + poll loop and clears the session card —
    /// used when a phone goes live, and to reopen the gate fresh.
    fn stop_pairing_surfaces(&mut self) {
        self.state.pairing_session = None;
        self.state.pairing_status.clear();
        self.state.pairing_seq += 1;
        self.stop_pairing_session();
    }

    // ------------------------------------------------------------------
    // QR pairing: desktop shows a QR, the phone scans and posts.
    // ------------------------------------------------------------------

    /// Binds the pairing listener off the UI thread; the endpoint and
    /// one-time code land in `state.pairing_session` for the gate's QR.
    fn start_pairing_session(&mut self, cx: &mut Context<Self>) {
        self.state.pairing_seq += 1;
        let seq = self.state.pairing_seq;
        self.state.pairing_session = None;
        self.state.pairing_status = "Starting pairing listener…".into();
        cx.spawn(async move |this, cx| {
            let listener = cx
                .background_executor()
                .spawn(async { PairingListener::start() })
                .await;
            _ = this.update(cx, |ws, cx| {
                if ws.state.pairing_seq != seq || ws.state.live {
                    return;
                }
                match listener {
                    Ok(listener) => {
                        let endpoint = listener.endpoint().to_string();
                        let offline = endpoint.starts_with("127.");
                        // A loopback endpoint scans as a perfectly valid
                        // QR the phone can never reach; show the waiting
                        // card instead and let the IP watcher rebind once
                        // a LAN address returns.
                        if !offline {
                            ws.state.pairing_session = Some(crate::state::PairingSession {
                                endpoint: endpoint.clone(),
                                code: listener.code().to_string(),
                            });
                        }
                        ws.state.pairing_status = if offline {
                            "No LAN address — join Wi-Fi, or plug in via USB".into()
                        } else {
                            "Waiting for your phone…".into()
                        };
                        let bound_ip = endpoint
                            .rsplit_once(':')
                            .map(|(ip, _)| ip.to_string())
                            .unwrap_or_else(|| endpoint.clone());
                        ws.pairing = Some(listener);
                        ws.poll_pairing(seq, cx);
                        ws.watch_pairing_ip(seq, bound_ip, cx);
                    }
                    Err(e) => {
                        ws.state.pairing_status =
                            format!("Pairing listener failed ({e}) — pair manually below");
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Shuts the pairing listener down (idempotent).
    fn stop_pairing_session(&mut self) {
        if let Some(listener) = self.pairing.take() {
            listener.stop();
        }
    }

    /// While a pairing session is up, re-checks the LAN address every 5 s
    /// (Wi-Fi flaps, DHCP renews and sleep/wake all change it under a bound
    /// listener) and rebinds the session so the QR never advertises an
    /// address the phone cannot reach. `lan_ip` is a routing-table lookup
    /// — a UDP "connect" that sends nothing — so polling is free. Retires
    /// with the session's `pairing_seq`; the restarted session starts its
    /// own watcher.
    fn watch_pairing_ip(&mut self, seq: u64, bound_ip: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                let lan = cx
                    .background_executor()
                    .spawn(async { lan_ip().to_string() })
                    .await;
                let exit = this
                    .update(cx, |ws, cx| {
                        if ws.state.pairing_seq != seq || ws.state.live {
                            // Session replaced or gate down: retire.
                            return true;
                        }
                        if lan == bound_ip {
                            return false;
                        }
                        // Network changed under the QR: rebind with a
                        // fresh code + port. `start_pairing_session` bumps
                        // `pairing_seq`, retiring this watcher and the old
                        // poll loop; the new session restarts both.
                        ws.state.pairing_status = "Network changed — scan the new QR".into();
                        ws.start_pairing_session(cx);
                        true
                    })
                    .unwrap_or(true);
                if exit {
                    return;
                }
            }
        })
        .detach();
    }

    /// Polls the pairing channel every 250 ms until the gate leaves, a
    /// payload arrives, or the listener reports closure (which restarts
    /// the session with a fresh QR); guarded by `pairing_seq` so stale
    /// loops die.
    fn poll_pairing(&mut self, seq: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let mut keep_polling = false;
                _ = this.update(cx, |ws, cx| {
                    if ws.state.pairing_seq != seq || ws.state.live {
                        return;
                    }
                    match ws.pairing.as_ref().and_then(PairingListener::try_event) {
                        Some(PairingEvent::Payload(payload)) => ws.accept_pairing(payload, cx),
                        // Dead listener under a live QR (wrong-code cap,
                        // accept failure): regenerate code + port so the
                        // next scan reaches something.
                        Some(PairingEvent::Closed {
                            paired: false,
                            reason,
                        }) => {
                            ws.state.pairing_status =
                                format!("Pairing code expired ({reason}) — scan the new QR");
                            ws.start_pairing_session(cx);
                        }
                        // A paired closure trails the payload
                        // `accept_pairing` already consumed — nothing to do.
                        Some(PairingEvent::Closed { paired: true, .. }) => {}
                        None => keep_polling = ws.pairing.is_some(),
                    }
                });
                if !keep_polling {
                    return;
                }
            }
        })
        .detach();
    }

    /// A phone scanned the QR: stop listening, dial back with retries
    /// (the phone needs a beat to bind its own service), and connect.
    /// The gate stays up with a status line until the connect lands —
    /// `apply_live_backend` swaps it for the shell on success.
    fn accept_pairing(&mut self, payload: PairingPayload, cx: &mut Context<Self>) {
        let device_id = if payload.device_id.trim().is_empty() {
            payload.name.clone()
        } else {
            payload.device_id.clone()
        };
        self.stop_pairing_session();
        self.state.pairing_session = None;
        self.state.pairing_status = "Phone scanned the code — connecting…".into();
        self.connect_with_retries(payload.base_url(), device_id, payload.token, None, 12, cx);
    }

    /// Rescan now (the gate's Scan button): restarts the watcher loop,
    /// whose first pass runs immediately.
    pub fn scan_devices(&mut self, cx: &mut Context<Self>) {
        self.start_device_watch(Duration::ZERO, cx);
    }

    /// Device watcher: while the onboarding gate is up (offline), repeats
    /// every 3 s with a full mDNS + adb scan feeding its Discovered
    /// devices list — a USB plug-in raises the phone's allow prompt on its
    /// own. Restarting it (or going live) retires the previous loop via
    /// `scan_seq`, and it never touches `connect_seq`, so refreshes cannot
    /// cancel an in-flight approval.
    fn start_device_watch(&mut self, delay: Duration, cx: &mut Context<Self>) {
        self.state.scan_seq += 1;
        let seq = self.state.scan_seq;
        if !self.state.connecting {
            self.state.device_scan = "Scanning for devices…".into();
        }
        cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            loop {
                let active = this
                    .update(cx, |ws, _| ws.state.scan_seq == seq && !ws.state.live)
                    .unwrap_or(false);
                if !active {
                    return;
                }
                let found =
                    cx.background_executor()
                        .spawn(async move {
                            let report = android18_transport::discover(Duration::from_secs(2));
                            let mut devices = report.devices;
                            for device in &mut devices {
                                let tunnel = device.adb_serial.clone().and_then(|serial| {
                                    android18_transport::adb_forward(&serial).ok()
                                });
                                if let Some(port) = tunnel {
                                    device.port = port;
                                }
                            }
                            (devices, report.unauthorized.len())
                        })
                        .await;
                _ = this.update(cx, |ws, cx| {
                    if ws.state.scan_seq == seq && !ws.state.live {
                        ws.apply_scan(found.0, found.1, cx);
                    }
                });
                cx.background_executor().timer(Duration::from_secs(3)).await;
            }
        })
        .detach();
    }

    /// Folds one scan pass into state and raises the USB allow prompt for
    /// newly plugged phones.
    fn apply_scan(
        &mut self,
        found: Vec<android18_transport::DiscoveredDevice>,
        unauthorized: usize,
        cx: &mut Context<Self>,
    ) {
        let usb_now: HashSet<String> = found
            .iter()
            .filter_map(|device| device.adb_serial.clone())
            .collect();
        // Unplugged serials may prompt again when they come back.
        self.state
            .usb_prompted
            .retain(|serial| usb_now.contains(serial));
        let candidates: Vec<DeviceCandidate> = found
            .into_iter()
            .map(|found| DeviceCandidate {
                paired: android18_transport::load_token(&found.id).is_some(),
                base_url: found.base_url().unwrap_or_default(),
                code_pairing: found.code_pairing,
                name: found.name,
                id: found.id,
                source: match found.source {
                    DiscoverySource::Mdns => DeviceSource::Wlan,
                    DiscoverySource::Usb => DeviceSource::Usb,
                },
                adb_serial: found.adb_serial,
            })
            .collect();
        let fresh_usb = candidates
            .iter()
            .find(|candidate| {
                !candidate.base_url.is_empty()
                    && candidate
                        .adb_serial
                        .as_ref()
                        .is_some_and(|serial| !self.state.usb_prompted.contains(serial))
            })
            .cloned();
        self.state.candidates = candidates;
        if !self.state.connecting {
            self.state.device_scan = match self.state.candidates.len() {
                0 => "No devices yet — plug in via USB, scan the QR, or start the \
                      service on your phone"
                    .into(),
                count => format!("{count} device(s) found"),
            };
            if unauthorized > 0 {
                self.state.device_scan = format!(
                    "{} · {unauthorized} attached but unauthorized — accept the \
                     USB-debugging prompt on that device",
                    self.state.device_scan
                );
            }
        }
        if let Some(candidate) = fresh_usb
            && !self.state.connecting
            && let Some(serial) = candidate.adb_serial.clone()
        {
            self.state.usb_prompted.insert(serial);
            // The gate is already up — just raise the phone's allow prompt.
            self.begin_pairing(candidate, None, cx);
        }
        cx.notify();
    }

    /// Connect click on a discovered row. A saved pairing token (paired
    /// before) connects directly; otherwise the phone is asked — its
    /// 6-char code inline when it advertises one, an Allow prompt if not.
    pub fn connect_candidate(&mut self, candidate: &DeviceCandidate, cx: &mut Context<Self>) {
        if candidate.base_url.is_empty() {
            self.state.device_scan = "No tunnel for this device — check USB/adb".into();
            cx.notify();
            return;
        }
        if let Some(token) =
            android18_transport::load_token(&candidate.id).filter(|token| token.len() >= 8)
        {
            self.connect_with_retries(
                candidate.base_url.clone(),
                candidate.id.clone(),
                token,
                candidate.adb_serial.clone(),
                1,
                cx,
            );
            return;
        }
        self.begin_pairing(candidate.clone(), None, cx);
    }

    /// Row click when the phone wants its code: reveal the inline input.
    pub fn open_code_entry(
        &mut self,
        candidate_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.code_target = Some(candidate_id.to_string());
        self.device_code_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.device_code_input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// Submits the inline code once it is a complete, valid 6 characters.
    fn submit_code(&mut self, cx: &mut Context<Self>) {
        if self.state.connecting {
            return;
        }
        let Some(code) =
            android18_transport::normalize_code(self.device_code_input.read(cx).value().as_ref())
        else {
            return;
        };
        let Some(target) = self.state.code_target.clone() else {
            return;
        };
        let Some(candidate) = self
            .state
            .candidates
            .iter()
            .find(|candidate| candidate.id == target)
            .cloned()
        else {
            return;
        };
        self.begin_pairing(candidate, Some(code), cx);
    }

    /// The one pairing request path. `code` rides in the body (phone grants
    /// at once); without it the phone shows Allow/Deny. USB rows first
    /// fire the connect intent over `adb` — the phone's service only
    /// starts once its user taps Allow, so the request retries until the
    /// tunnel answers. On a grant the token connects and is saved by
    /// `connect_with_retries`, so later connects take the fast path.
    fn begin_pairing(
        &mut self,
        candidate: DeviceCandidate,
        code: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.state.connecting = true;
        self.state.connect_seq += 1;
        let seq = self.state.connect_seq;
        let usb_serial = candidate.adb_serial.clone();
        self.state.device_scan = if usb_serial.is_some() {
            format!("Look at {} — tap Allow to connect", candidate.name)
        } else if code.is_some() {
            format!("Checking the code with {}…", candidate.name)
        } else {
            format!("Asking {} to allow this computer…", candidate.name)
        };
        cx.notify();
        let url = candidate.base_url.clone();
        let name = desktop_name();
        cx.spawn(async move |this, cx| {
            let query = url.clone();
            let grant = cx
                .background_executor()
                .spawn(async move {
                    if let Some(serial) = usb_serial.as_deref() {
                        android18_transport::adb_launch_connect(serial, &name)
                            .map_err(PairRequestError::Unreachable)?;
                    }
                    // The phone drops its prompt after 60s and answers
                    // 408; the client deadline sits just past it so the
                    // phone's clearer copy wins. USB retries while the
                    // service is still starting (or was denied, and so
                    // never starts) for the same window.
                    let deadline = std::time::Instant::now() + Duration::from_secs(70);
                    loop {
                        match android18_transport::request_pairing(
                            &query,
                            &name,
                            code.as_deref(),
                            Duration::from_secs(65),
                        ) {
                            Err(PairRequestError::Unreachable(_))
                                if usb_serial.is_some() && std::time::Instant::now() < deadline =>
                            {
                                std::thread::sleep(Duration::from_secs(1));
                            }
                            Err(PairRequestError::Unreachable(_)) if usb_serial.is_some() => {
                                return Err(PairRequestError::NoAnswer);
                            }
                            other => return other,
                        }
                    }
                })
                .await;
            _ = this.update(cx, |ws, cx| {
                if ws.state.connect_seq != seq {
                    return;
                }
                match grant {
                    Ok(PairGrant {
                        name,
                        device_id,
                        token,
                    }) => {
                        let id = if device_id.trim().is_empty() {
                            name.clone()
                        } else {
                            device_id
                        };
                        ws.state.device_scan = format!("Approved — connecting to {name}…");
                        cx.notify();
                        ws.connect_with_retries(
                            url,
                            id,
                            token,
                            candidate.adb_serial.clone(),
                            1,
                            cx,
                        );
                    }
                    Err(e) => {
                        ws.state.connecting = false;
                        ws.state.device_scan = match e {
                            PairRequestError::Denied => {
                                format!("{} denied the request", candidate.name)
                            }
                            PairRequestError::NoAnswer => {
                                "No answer on the phone — try again".into()
                            }
                            PairRequestError::WrongCode => {
                                "Wrong code — check the one on your phone".into()
                            }
                            PairRequestError::RateLimited => {
                                "Too many wrong codes — wait 30 seconds and retry".into()
                            }
                            PairRequestError::Unreachable(detail) => {
                                format!("Pair request failed: {detail}")
                            }
                        };
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Advanced manual pairing: address + optional 6-char code. With a
    /// code the phone grants at once; without, it asks to allow.
    pub fn connect_manual(&mut self, cx: &mut Context<Self>) {
        let url = self.device_url_input.read(cx).value().trim().to_string();
        if url.is_empty() {
            self.state.device_scan = "Enter the phone's address (http://ip:port)".into();
            cx.notify();
            return;
        }
        let typed = self.manual_code_input.read(cx).value().trim().to_string();
        let code = android18_transport::normalize_code(&typed);
        if !typed.is_empty() && code.is_none() {
            self.state.device_scan = "The pair code is 6 characters, like 0X1D8C".into();
            cx.notify();
            return;
        }
        let candidate = DeviceCandidate {
            id: String::new(),
            name: "the phone".into(),
            source: DeviceSource::Wlan,
            base_url: url,
            paired: false,
            adb_serial: None,
            code_pairing: code.is_some(),
        };
        self.begin_pairing(candidate, code, cx);
    }

    /// Probes `url` with `token` off the UI thread; on success the backend
    /// swaps in, the token persists to the state file, and listings refresh.
    /// `via_serial` records the USB tunnel a connection rode on, if any.
    /// `attempts` > 1 retries once per second — QR pairing needs a beat
    /// while the phone binds its service after posting.
    fn connect_with_retries(
        &mut self,
        url: String,
        device_id: String,
        token: String,
        via_serial: Option<String>,
        attempts: u32,
        cx: &mut Context<Self>,
    ) {
        self.state.connecting = true;
        self.state.device_scan = format!("Connecting to {url}…");
        self.state.connect_seq += 1;
        let seq = self.state.connect_seq;
        let saved_token = token.clone();
        cx.spawn(async move |this, cx| {
            let attempt = cx
                .background_executor()
                .spawn(async move {
                    let mut last = None;
                    for try_index in 0..attempts.max(1) {
                        if try_index > 0 {
                            std::thread::sleep(Duration::from_secs(1));
                        }
                        let attempt = async {
                            let backend = HttpDevice::new(&url, &token)?;
                            let info = backend.device_info().await?;
                            Ok::<_, android18_core::domain::DeviceError>((backend, info))
                        };
                        match attempt.await {
                            Ok(pair) => return Ok(pair),
                            Err(e) => last = Some(e),
                        }
                    }
                    Err(last.unwrap_or(android18_core::domain::DeviceError::Io(
                        "connection attempts exhausted".into(),
                    )))
                })
                .await;
            _ = this.update(cx, |ws, cx| {
                if ws.state.connect_seq != seq {
                    return;
                }
                match attempt {
                    Ok((backend, info)) => {
                        // Persist under both ids: scan rows look the token
                        // up by candidate id (`adb:<serial>`), launch
                        // reconnect by the phone's real id.
                        let _ = android18_transport::save_token(&info.id, &saved_token);
                        if !device_id.is_empty() {
                            let _ = android18_transport::save_token(&device_id, &saved_token);
                        }
                        if let Some(serial) = via_serial.as_deref() {
                            let _ = android18_transport::save_usb_serial(&info.id, serial);
                        }
                        backend.set_persistence_key(&info.id);
                        ws.apply_live_backend(Arc::new(backend), info, cx);
                    }
                    Err(e) => {
                        ws.state.connecting = false;
                        ws.state.device_scan = format!("Connection failed: {e}");
                        ws.state.error = Some(format!("Pairing failed: {e}"));
                        // A scanned QR spent its one-shot listener: offer a
                        // fresh code instead of a dead gate.
                        if !ws.state.live && ws.pairing.is_none() {
                            ws.start_pairing_session(cx);
                        }
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Installs a live backend: persists last-device, resets navigation, and
    /// refreshes every derived surface.
    fn apply_live_backend(&mut self, backend: SharedBackend, info: Device, cx: &mut Context<Self>) {
        let _ = android18_transport::save_last_device(&info);
        // A silent reconnect to the same phone keeps the opened folder.
        let same_device = self.state.live && self.state.device_info.id == info.id;
        self.state.device = backend;
        self.state.device_info = info.clone();
        self.state.live = true;
        self.state.connecting = false;
        self.state.scan_seq += 1; // retire the device watcher
        self.state.usb_prompted.clear();
        self.state.code_target = None;
        self.state.server_open = false;
        self.state.pairing_session = None;
        self.state.pairing_status.clear();
        self.state.candidates.clear();
        self.state.device_scan.clear();
        self.state.pairing_seq += 1; // retire any pairing poll loop
        self.stop_pairing_session();
        // Fresh device: drop queued rows and stop live runners (their
        // `.part` files stay behind, so a retry resumes where this stopped).
        self.state.transfers.clear();
        self.download_controls.clear();
        self.upload_controls.clear();
        self.thumbnails.clear();
        self.pending_thumbs.clear();
        self.failed_thumbs.clear();
        self.thumb_order.clear();
        self.thumb_queue.clear();
        if !same_device {
            self.state.cwd = STORAGE_ROOT.to_string();
            self.state.history = vec![STORAGE_ROOT.to_string()];
            self.state.history_index = 0;
        }
        self.state.selected = None;
        self.state.selected_preview = None;
        self.state.selected_preview_image = None;
        if !same_device {
            self.state.quick_filter = None;
        }
        self.state.search = None;
        self.state.error = None;
        self.refresh_list(cx);
        self.refresh_storage(cx);
        self.start_heartbeat(cx);
        self.show_toast(format!("Connected to {}", info.name), cx);
    }

    /// Silent auto-reconnect to the last paired phone at launch. USB-attached
    /// phones re-create their `adb forward` tunnel first — the phone's
    /// self-reported LAN URL is unreachable from the host (emulator NAT).
    fn try_reconnect(&mut self, cx: &mut Context<Self>) {
        let Some(last) = android18_transport::load_last_device() else {
            return;
        };
        let Some(token) = android18_transport::load_token(&last.id) else {
            return;
        };
        let usb_serial = android18_transport::load_usb_serial(&last.id);
        self.state.connect_seq += 1;
        let seq = self.state.connect_seq;
        cx.spawn(async move |this, cx| {
            let attempt = cx
                .background_executor()
                .spawn(async move {
                    let url = match usb_serial.as_deref() {
                        Some(serial) => {
                            let port = android18_transport::adb_forward(serial)
                                .map_err(android18_core::domain::DeviceError::Offline)?;
                            format!("http://127.0.0.1:{port}")
                        }
                        None => last.base_url.clone(),
                    };
                    let backend = HttpDevice::new(&url, &token)?;
                    let info = backend.device_info().await?;
                    Ok::<_, android18_core::domain::DeviceError>((backend, info))
                })
                .await;
            if let Ok((backend, info)) = attempt {
                _ = this.update(cx, |ws, cx| {
                    if ws.state.connect_seq == seq {
                        backend.set_persistence_key(&info.id);
                        ws.apply_live_backend(Arc::new(backend), info, cx);
                    }
                });
            }
        })
        .detach();
    }

    /// Forgets the paired phone and returns to the offline placeholder.
    pub fn disconnect_device(&mut self, cx: &mut Context<Self>) {
        let _ = android18_transport::clear_last_device();
        self.go_offline("Disconnected", cx);
    }

    /// Shared offline reset behind both a manual disconnect and the
    /// heartbeat noticing the phone is gone: wipes every phone-derived
    /// surface (listings, tree, dashboard, selection, shell) and puts the
    /// onboarding gate back up with a fresh pairing session and device
    /// watch. The last-device record is the caller's call — a vanished
    /// phone keeps it so launch auto-reconnect still works.
    fn go_offline(&mut self, toast: &str, cx: &mut Context<Self>) {
        // A phone that just left must not be re-prompted by the watcher
        // while it is still plugged in.
        if let Some(serial) = android18_transport::load_usb_serial(&self.state.device_info.id) {
            self.state.usb_prompted.insert(serial);
        }
        self.state.clear_phone_data();
        self.thumbnails.clear();
        self.pending_thumbs.clear();
        self.failed_thumbs.clear();
        self.thumb_order.clear();
        self.thumb_queue.clear();
        self.pending_inspect = None;
        self.terminal_history_cursor = None;
        let mut shell = self.shell.lock().expect("shell mutex");
        *shell = Some(ShellSession::new("phone"));
        if let Some(session) = shell.as_ref() {
            self.state.transcript = session.welcome(&self.state.device_info);
        }
        drop(shell);
        // Back on the gate: retire the old QR session, start a fresh one,
        // and resume the device watch (the delay skips the first scan
        // while the just-left phone settles).
        self.stop_pairing_surfaces();
        self.start_pairing_session(cx);
        self.start_device_watch(Duration::from_secs(3), cx);
        // Queued transfers stay: their runners fail on the next chunk and
        // surface the offline error in the transfers drawer.
        self.show_toast(toast.to_string(), cx);
        cx.notify();
    }

    /// Live-connection watchdog: pings `/info` every few seconds and drops
    /// to offline when the phone stops answering (server stopped, Wi-Fi
    /// gone, cable pulled) or rejects the token. The pings also tell the
    /// phone a desktop is connected. Retired via `heartbeat_seq`.
    fn start_heartbeat(&mut self, cx: &mut Context<Self>) {
        self.state.heartbeat_seq += 1;
        let seq = self.state.heartbeat_seq;
        let device = self.state.device.clone();
        cx.spawn(async move |this, cx| {
            let mut misses = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                let current = this
                    .update(cx, |ws, _| ws.state.heartbeat_seq == seq && ws.state.live)
                    .unwrap_or(false);
                if !current {
                    return;
                }
                let probe = device.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { probe.device_info().await })
                    .await;
                match result {
                    Ok(_) => misses = 0,
                    Err(error) => {
                        misses += 1;
                        if crate::state::heartbeat_lost(&error, misses) {
                            _ = this.update(cx, |ws, cx| {
                                if ws.state.heartbeat_seq == seq && ws.state.live {
                                    ws.go_offline("Phone disconnected", cx);
                                }
                            });
                            return;
                        }
                    }
                }
            }
        })
        .detach();
    }

    /// Active search provider: phone-backed when live, heuristic otherwise.
    fn search_provider(&self) -> SharedSearch {
        if self.state.live {
            Arc::new(HttpSearchProvider::new(
                self.state.device_info.base_url.clone(),
                self.state.token(),
            ))
        } else {
            Arc::new(HeuristicSearchProvider)
        }
    }

    /// Native-menu About modal (`ui::modals::about`).
    pub fn toggle_about(&mut self, cx: &mut Context<Self>) {
        self.state.about_open = !self.state.about_open;
        cx.notify();
    }

    /// Settings modal (⌘, / app menu): persisted default view, theme
    /// (locked to Light until a dark token set exists), connection/token,
    /// and a two-step forget control.
    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.state.settings_open = !self.state.settings_open;
        self.forget_armed = false;
        cx.notify();
    }

    pub fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.state.settings_open = false;
        self.forget_armed = false;
        cx.notify();
    }

    /// Applies a new launch default view immediately and persists it. The
    /// browser's ⌘G toggle deliberately does *not* write settings — the
    /// default only changes here.
    pub fn set_default_view(&mut self, view: DefaultView, cx: &mut Context<Self>) {
        self.settings.default_view = view;
        self.state.view_mode = view.into();
        self.persist_settings(cx);
        cx.notify();
    }

    /// Copies the live pairing token to the system clipboard.
    pub fn copy_pairing_token(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.state.token()));
        self.show_toast("Pairing token copied", cx);
    }

    /// Deletes the paired phone's saved token and returns to the
    /// offline placeholder; the phone must scan the desktop QR to pair
    /// again.
    pub fn forget_device(&mut self, cx: &mut Context<Self>) {
        let id = self.state.device_info.id.clone();
        if !id.is_empty() {
            let _ = android18_transport::delete_token(&id);
        }
        self.disconnect_device(cx);
        self.state.settings_open = false;
        self.forget_armed = false;
        self.show_toast("Pairing forgotten", cx);
    }

    /// Records the window frame (debounced 500 ms) so the next launch
    /// reopens where it was left.
    fn schedule_window_save(&mut self, window: &Window, cx: &mut Context<Self>) {
        let (maximized, bounds) = match window.window_bounds() {
            WindowBounds::Windowed(b) => (false, b),
            WindowBounds::Maximized(b) => (true, b),
            // Fullscreen is transient; keep the previous frame.
            WindowBounds::Fullscreen(_) => return,
        };
        let state = WindowState {
            x: f32::from(bounds.origin.x) as i32,
            y: f32::from(bounds.origin.y) as i32,
            width: f32::from(bounds.size.width) as i32,
            height: f32::from(bounds.size.height) as i32,
            maximized,
        };
        self.pending_window_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            _ = this.update(cx, |ws, cx| {
                if let Some(state) = state.validated()
                    && ws.settings.window != Some(state)
                {
                    ws.settings.window = Some(state);
                    ws.persist_settings(cx);
                }
            });
        }));
    }

    /// Writes settings off the UI thread. Cosmetic persistence: failures
    /// are dropped silently and defaults come back next launch.
    fn persist_settings(&self, cx: &mut Context<Self>) {
        let settings = self.settings;
        let Some(path) = settings_path() else {
            return;
        };
        cx.background_spawn(async move {
            let _ = settings.write_to(&path);
        })
        .detach();
    }

    pub fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.state.live {
            return;
        }
        self.state.search_open = true;
        let handle = self.search_input.focus_handle(cx);
        handle.focus(window, cx);
        cx.notify();
    }

    pub fn close_search(&mut self, cx: &mut Context<Self>) {
        self.state.search_open = false;
        cx.notify();
    }

    /// §14.1: Escape peels one layer off, innermost first.
    pub fn escape(&mut self, cx: &mut Context<Self>) {
        if self.state.about_open {
            self.state.about_open = false;
        } else if self.state.settings_open {
            self.state.settings_open = false;
            self.forget_armed = false;
        } else if self.state.server_open {
            self.close_server_sheet(cx);
            return;
        } else if self.state.new_folder_open {
            self.state.new_folder_open = false;
        } else if self.state.rename_open {
            self.state.rename_open = false;
        } else if self.state.clipboard.is_some() {
            // Cancel a staged cut/copy before falling through to panels.
            self.cancel_clipboard(cx);
            return;
        } else if self.state.search_open {
            self.state.search_open = false;
        } else if self.state.transfers_open {
            self.state.transfers_open = false;
        } else if self.state.terminal_open {
            self.state.terminal_open = false;
            self.state.terminal_maximized = false;
        } else if self.state.inspector_open {
            self.state.inspector_open = false;
            self.cancel_pending_inspect();
        } else {
            self.state.error = None;
        }
        cx.notify();
    }
    /// Runs the AI query over a full-tree snapshot in the background.
    pub fn run_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value().to_string();
        if query.trim().is_empty() {
            return;
        }
        self.state.search_seq += 1;
        let seq = self.state.search_seq;
        self.state.search_loading = true;
        cx.notify();
        let device = self.state.device.clone();
        let cwd = self.state.cwd.clone();
        let token = self.state.token();
        let provider = self.search_provider();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // Best-effort snapshot for the offline fallback.
                    let entries = device.walk(STORAGE_ROOT, &token).await.unwrap_or_default();
                    provider.search(&query, &entries, &cwd).await
                })
                .await;
            _ = this.update(cx, |ws, cx| {
                if ws.state.search_seq != seq {
                    return;
                }
                ws.state.search_loading = false;
                match result {
                    Ok(result) => {
                        ws.state.search = Some(result);
                        ws.state.error = None;
                    }
                    Err(e) => ws.state.error = Some(format!("Search failed: {e}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Executes one shell line off the UI thread; the session is taken out of
    /// its slot while the command runs and returned when the reply lands.
    /// Mutating commands refresh the listing + tree, and a `cd` carries the
    /// browser along with the shell's working directory.
    pub fn submit_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let line = self.terminal_input.read(cx).value().to_string();
        if line.trim().is_empty() {
            return;
        }
        self.state.terminal_open = true;
        let mut shell = self.shell.lock().expect("shell mutex");
        let Some(mut session) = shell.take() else {
            // A command is already in flight; ignore instead of queueing.
            return;
        };
        let prompt_cwd = session.cwd().to_string();
        drop(shell);

        // The line is taken: clear the prompt so the next command starts empty.
        self.terminal_input
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.terminal_history_cursor = None;

        let prompt = session.prompt();
        self.state
            .transcript
            .push(ShellLine::new(OutputKind::Cmd, format!("{prompt}{line}")));
        self.state.error = None;
        cx.notify();

        let device = self.state.device.clone();
        let token = self.state.token();
        let provider = self.search_provider();
        cx.spawn(async move |this, cx| {
            let (reply, session) = cx
                .background_executor()
                .spawn(async move {
                    let reply = session.execute(&line, &*device, &token, &*provider).await;
                    (reply, session)
                })
                .await;
            let mutated = reply.mutated;
            let follows_cwd = session.cwd() != prompt_cwd;
            _ = this.update(cx, |ws, cx| {
                if reply.clear {
                    ws.state.transcript.clear();
                } else {
                    ws.state.transcript.extend(reply.lines);
                }
                *ws.shell.lock().expect("shell mutex") = Some(session);
                ws.terminal_history_cursor = None;
                if mutated {
                    ws.refresh_list(cx);
                    ws.refresh_storage(cx);
                }
                if follows_cwd {
                    let cwd = ws
                        .shell
                        .lock()
                        .expect("shell mutex")
                        .as_ref()
                        .map(|session| session.cwd().to_string());
                    if let Some(cwd) = cwd {
                        ws.navigate(cwd, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// ↑/↓ recall over the shell history (§11.3). ↑ walks back from the
    /// newest line; ↓ walks forward, clearing the prompt past the oldest.
    pub fn terminal_history(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let shell = self.shell.lock().expect("shell mutex");
        let Some(session) = shell.as_ref() else {
            return; // a command is in flight; its history is not final yet
        };
        let history = session.history();
        if history.is_empty() {
            return;
        }
        let next = match (forward, self.terminal_history_cursor) {
            (false, None) => Some(history.len() - 1),
            (false, Some(index)) => index.checked_sub(1),
            (true, Some(index)) if index + 1 < history.len() => Some(index + 1),
            (true, _) => None,
        };
        let line = match next {
            Some(index) => history[index].clone(),
            None => String::new(),
        };
        drop(shell);
        self.terminal_history_cursor = next;
        self.terminal_input
            .update(cx, |state, cx| state.set_value(line, window, cx));
        cx.notify();
    }
    pub fn open_new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.state.live {
            return;
        }
        self.state.new_folder_open = true;
        let handle = self.new_folder_input.focus_handle(cx);
        handle.focus(window, cx);
        cx.notify();
    }

    pub fn close_new_folder(&mut self, cx: &mut Context<Self>) {
        self.state.new_folder_open = false;
        cx.notify();
    }

    /// Creates a folder under the cwd from the new-folder input.
    pub fn create_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.new_folder_input.read(cx).value().to_string();
        if name.trim().is_empty() {
            self.state.error = Some("Folder name must not be empty".into());
            cx.notify();
            return;
        }
        let target = format!("{}/{}", self.state.cwd.trim_end_matches('/'), name.trim());
        self.new_folder_input
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.state.new_folder_open = false;
        self.run_mutation(target, Mutation::MkDir, cx);
    }

    /// Deletes every selected entry (recursive for directories) in one
    /// background pass; the listing refreshes once at the end.
    pub fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            return;
        }
        let device = self.state.device.clone();
        let token = self.state.token();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut first_error = None;
                    for path in paths {
                        if let Err(e) = device.remove(&path, &token).await
                            && first_error.is_none()
                        {
                            first_error = Some(e);
                        }
                    }
                    first_error
                })
                .await;
            _ = this.update(cx, |ws, cx| {
                match result {
                    Some(e) => ws.state.error = Some(e.to_string()),
                    None => ws.state.error = None,
                }
                ws.state.clear_selection();
                ws.state.selected_preview = None;
                ws.state.selected_preview_image = None;
                ws.refresh_list(cx);
                ws.refresh_storage(cx);
            });
        })
        .detach();
    }

    /// Opens the rename modal for the anchor entry.
    pub fn open_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.state.selected_entry().cloned() else {
            return;
        };
        let name = entry.name.clone();
        self.state.rename_open = true;
        self.rename_input
            .update(cx, |state, cx| state.set_value(name, window, cx));
        let handle = self.rename_input.focus_handle(cx);
        handle.focus(window, cx);
        cx.notify();
    }

    pub fn close_rename(&mut self, cx: &mut Context<Self>) {
        self.state.rename_open = false;
        cx.notify();
    }

    /// Renames the selected entry from the rename input (`POST /mv`).
    pub fn rename_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.state.selected_entry().cloned() else {
            self.state.rename_open = false;
            cx.notify();
            return;
        };
        let name = self.rename_input.read(cx).value().to_string();
        let name = name.trim();
        if name.is_empty() || name.contains('/') || name == entry.name {
            self.state.error = Some("Enter a new name (no slashes)".into());
            cx.notify();
            return;
        }
        let Some(parent) = parent_of(&entry.path) else {
            return;
        };
        let target = format!("{parent}/{name}");
        self.rename_input
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.state.rename_open = false;
        self.run_mutation(entry.path, Mutation::Mv(target), cx);
    }

    /// Stages the current selection for a later paste (§7.1): Copy leaves
    /// the originals in place, Cut removes them once the paste lands.
    pub fn copy_selected(&mut self, cx: &mut Context<Self>) {
        self.stage_clipboard(ClipboardMode::Copy, cx);
    }

    pub fn cut_selected(&mut self, cx: &mut Context<Self>) {
        self.stage_clipboard(ClipboardMode::Cut, cx);
    }

    fn stage_clipboard(&mut self, mode: ClipboardMode, cx: &mut Context<Self>) {
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            return;
        }
        let count = paths.len();
        self.state.clipboard = Some(Clipboard { mode, paths });
        self.show_toast(
            format!(
                "{count} item{} {} — open a folder and paste",
                if count == 1 { "" } else { "s" },
                mode.verb()
            ),
            cx,
        );
    }

    /// Drops the staged cut/copy without pasting (toolbar ✕ or Escape).
    pub fn cancel_clipboard(&mut self, cx: &mut Context<Self>) {
        if self.state.clipboard.take().is_some() {
            cx.notify();
        }
    }

    /// Pastes the staged paths into the current folder (§7.1): Cut rides
    /// the guarded `mv` loop, Copy queues one `cp` per entry. Entries
    /// already in the destination are skipped; a folder never lands inside
    /// itself. The clipboard always empties, even on partial failure.
    pub fn paste_into_folder(&mut self, cx: &mut Context<Self>) {
        let Some(clipboard) = self.state.clipboard.clone() else {
            self.show_toast("Nothing staged — copy or cut files first", cx);
            return;
        };
        let dest = self.state.cwd.clone();
        let mut moves: Vec<(String, String)> = Vec::new();
        let mut copies: Vec<String> = Vec::new();
        for path in &clipboard.paths {
            if path == &dest || is_descendant(path, &dest) {
                self.state.error = Some("A folder can't be pasted into itself".into());
                cx.notify();
                return;
            }
            if parent_of(path).as_deref() == Some(dest.as_str()) {
                continue; // already there
            }
            match clipboard.mode {
                ClipboardMode::Cut => {
                    let name = self
                        .state
                        .entry_at(path)
                        .map(|e| e.name.clone())
                        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).to_string());
                    moves.push((
                        path.clone(),
                        format!("{}/{name}", dest.trim_end_matches('/')),
                    ));
                }
                ClipboardMode::Copy => copies.push(path.clone()),
            }
        }
        if moves.is_empty() && copies.is_empty() {
            self.state.clipboard = None;
            self.show_toast(format!("Already in {}", display_path(&dest)), cx);
            return;
        }
        self.state.clipboard = None;
        let label = display_path(&dest);
        match clipboard.mode {
            ClipboardMode::Cut => self.run_moves(moves, label, cx),
            ClipboardMode::Copy => self.run_copies(copies, label, cx),
        }
    }

    /// Sequential background `cp` loop (§7.1 paste); the listing and tree
    /// snapshot refresh once at the end. The selection stays — the
    /// originals are untouched by a copy.
    fn run_copies(&mut self, copies: Vec<String>, label: String, cx: &mut Context<Self>) {
        let device = self.state.device.clone();
        let token = self.state.token();
        cx.spawn(async move |this, cx| {
            let dest = label.clone();
            let (first_error, copied) = cx
                .background_executor()
                .spawn(async move {
                    let mut first_error = None;
                    let mut copied = 0;
                    for from in copies {
                        match device.cp(&from, &dest, &token).await {
                            Ok(()) => copied += 1,
                            Err(e) if first_error.is_none() => first_error = Some(e),
                            Err(_) => {}
                        }
                    }
                    (first_error, copied)
                })
                .await;
            _ = this.update(cx, |ws, cx| {
                if copied > 0 {
                    ws.show_toast(
                        format!(
                            "Copied {copied} item{} to {label}",
                            if copied == 1 { "" } else { "s" }
                        ),
                        cx,
                    );
                }
                match first_error {
                    Some(e) => ws.state.error = Some(e.to_string()),
                    None => ws.state.error = None,
                }
                ws.refresh_list(cx);
                ws.refresh_storage(cx);
            });
        })
        .detach();
    }

    /// Sequential background `mv` loop; the listing and the tree snapshot
    /// refresh once at the end, with a toast when everything landed.
    fn run_moves(&mut self, moves: Vec<(String, String)>, label: String, cx: &mut Context<Self>) {
        let device = self.state.device.clone();
        let token = self.state.token();
        cx.spawn(async move |this, cx| {
            let (first_error, moved) = cx
                .background_executor()
                .spawn(async move {
                    let mut first_error = None;
                    let mut moved = 0;
                    for (from, to) in moves {
                        match device.mv(&from, &to, &token).await {
                            Ok(()) => moved += 1,
                            Err(e) if first_error.is_none() => first_error = Some(e),
                            Err(_) => {}
                        }
                    }
                    (first_error, moved)
                })
                .await;
            _ = this.update(cx, |ws, cx| {
                if moved > 0 {
                    ws.show_toast(
                        format!(
                            "Moved {moved} item{} to {label}",
                            if moved == 1 { "" } else { "s" }
                        ),
                        cx,
                    );
                }
                match first_error {
                    Some(e) => ws.state.error = Some(e.to_string()),
                    None => ws.state.error = None,
                }
                ws.state.clear_selection();
                ws.state.selected_preview = None;
                ws.state.selected_preview_image = None;
                ws.refresh_list(cx);
                ws.refresh_storage(cx);
            });
        })
        .detach();
    }

    /// R4 thumbnails: declares which paths the visible rows want (nearest
    /// first). Called from the browser's render pass; it only mutates
    /// bookkeeping and starts background fetches — it never notifies, so
    /// rendering cannot re-arm itself. Arrivals re-render through a
    /// coalesced notify in `pump_thumbs`.
    pub fn want_thumbnails(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
        if !self.state.live {
            return;
        }
        // Replace the queue so rows scrolled out of view stop competing.
        self.thumb_queue = paths
            .into_iter()
            .filter(|p| {
                !self.thumbnails.contains_key(p)
                    && !self.pending_thumbs.contains(p)
                    && !self.failed_thumbs.contains(p)
            })
            .collect();
        self.pump_thumbs(cx);
    }

    /// Starts queued fetches up to [`MAX_THUMB_INFLIGHT`]; each completion
    /// pumps again, so the queue drains without depending on renders.
    fn pump_thumbs(&mut self, cx: &mut Context<Self>) {
        while self.pending_thumbs.len() < MAX_THUMB_INFLIGHT {
            let Some(key) = self.thumb_queue.pop_front() else {
                return;
            };
            if self.thumbnails.contains_key(&key)
                || self.pending_thumbs.contains(&key)
                || self.failed_thumbs.contains(&key)
            {
                continue;
            }
            self.pending_thumbs.insert(key.clone());
            let device = self.state.device.clone();
            let token = self.state.token();
            let fetch_key = key.clone();
            cx.spawn(async move |this, cx| {
                let bytes = cx
                    .background_executor()
                    .spawn(async move {
                        device
                            .thumb(&fetch_key, crate::thumb::THUMB_MAX_DIM, &token)
                            .await
                    })
                    .await;
                _ = this.update(cx, |ws, cx| {
                    ws.pending_thumbs.remove(&key);
                    match bytes.ok().and_then(|b| crate::thumb::decode(&b)) {
                        Some(rendered) => {
                            ws.thumbnails.insert(key.clone(), Arc::new(rendered));
                            ws.thumb_order.push_back(key);
                            // FIFO eviction, never a wholesale clear.
                            while ws.thumb_order.len() > MAX_THUMB_CACHE {
                                if let Some(old) = ws.thumb_order.pop_front() {
                                    ws.thumbnails.remove(&old);
                                }
                            }
                            ws.notify_thumbs(cx);
                        }
                        None => {
                            ws.failed_thumbs.insert(key);
                        }
                    }
                    ws.pump_thumbs(cx);
                });
            })
            .detach();
        }
    }

    /// Batches thumbnail arrivals: at most one re-render per 80 ms.
    fn notify_thumbs(&mut self, cx: &mut Context<Self>) {
        if self.thumb_notify_armed {
            return;
        }
        self.thumb_notify_armed = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(80))
                .await;
            _ = this.update(cx, |ws, cx| {
                ws.thumb_notify_armed = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// R4 pin toggle for the selected entry (desktop-local decoration).
    pub fn toggle_pin_selected(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.state.selected_entry().cloned() else {
            return;
        };
        let pinned = !entry.is_pinned;
        let device = self.state.device.clone();
        let path = entry.path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { device.set_pinned(&path, pinned).await })
                .await;
            _ = this.update(cx, |ws, cx| {
                match result {
                    Ok(()) => ws.state.error = None,
                    Err(e) => ws.state.error = Some(e.to_string()),
                }
                ws.refresh_list(cx);
            });
        })
        .detach();
    }

    /// R4 color tag for the selected entry (`None` clears).
    pub fn tag_selected(&mut self, tag: Option<ColorTag>, cx: &mut Context<Self>) {
        let Some(entry) = self.state.selected_entry().cloned() else {
            return;
        };
        let device = self.state.device.clone();
        let path = entry.path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { device.set_color_tag(&path, tag).await })
                .await;
            _ = this.update(cx, |ws, cx| {
                match result {
                    Ok(()) => ws.state.error = None,
                    Err(e) => ws.state.error = Some(e.to_string()),
                }
                ws.refresh_list(cx);
            });
        })
        .detach();
    }

    /// §7.3 download: enqueues one live chunked transfer per selected file
    /// into `~/Downloads/Android18/` (folders wait for R7).
    pub fn download_selected(&mut self, cx: &mut Context<Self>) {
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            return;
        }
        if !self.state.live {
            self.state.error = Some("Not connected — pair a phone first".into());
            cx.notify();
            return;
        }
        let mut queued_any = false;
        let mut saw_folder = false;
        // Rows picked from the dashboard recents live outside the current
        // listing, so resolve each path against the whole tree as well.
        let mut targets = self.state.entries.clone();
        for entry in &self.state.all_entries {
            if !targets.iter().any(|t| t.path == entry.path) {
                targets.push(entry.clone());
            }
        }
        for entry in targets {
            if !paths.contains(&entry.path) {
                continue;
            }
            if entry.dir {
                saw_folder = true;
                continue;
            }
            let id = self
                .state
                .transfers
                .iter()
                .map(|t| t.id)
                .max()
                .unwrap_or(100)
                + 1;
            let mut transfer = new_transfer(
                id,
                entry.name.clone(),
                TransferDirection::Download,
                entry.size.max(1),
                android18_core::fs::paths::display_path(&entry.path),
                true,
                ui::NOW_MS,
            );
            let _ = apply(&mut transfer, TransferEvent::Start);
            self.state.transfers.insert(0, transfer);
            self.spawn_download(
                DownloadJob {
                    id,
                    remote_path: entry.path.clone(),
                    file_name: entry.name.clone(),
                    size: entry.size,
                },
                cx,
            );
            queued_any = true;
        }
        if queued_any {
            self.state.transfers_open = true;
            self.show_toast("Downloading to ~/Downloads/Android18".to_string(), cx);
        } else if saw_folder {
            self.state.error = Some("Folder downloads arrive with R7 (zip streaming)".into());
            cx.notify();
        }
    }

    /// Runs a live download in the background: chunked `read_range` fetches
    /// appended to a `.part` file (resumable across launches), progress and
    /// completion reported through the transfer engine, and pause/resume/
    /// cancel commands forwarded from the transfer row.
    fn spawn_download(&mut self, job: DownloadJob, cx: &mut Context<Self>) {
        let (sender, receiver) = mpsc::channel::<DownloadCommand>();
        self.download_controls.insert(job.id, sender);
        let device = self.state.device.clone();
        let token = self.state.token();
        let id = job.id;
        let size = job.size;
        cx.spawn(async move |this, cx| {
            let setup = job.clone();
            let mut prepared = match cx
                .background_executor()
                .spawn(async move { download::prepare(&setup) })
                .await
            {
                Ok(prepared) => prepared,
                Err(message) => {
                    let _ = this.update(cx, |ws, cx| {
                        ws.apply_runner_report(id, TransferEvent::Fail { message }, cx);
                    });
                    return;
                }
            };
            let mut paused = false;
            let mut window_bytes = prepared.offset;
            let mut window_at = Instant::now();
            loop {
                match receiver.try_recv() {
                    Ok(DownloadCommand::Pause) => paused = true,
                    Ok(DownloadCommand::Resume) => paused = false,
                    Ok(DownloadCommand::Cancel) => {
                        let part = prepared.part.clone();
                        cx.background_executor()
                            .spawn(async move { download::discard(part) })
                            .await;
                        let _ = this.update(cx, |ws, cx| {
                            ws.download_controls.remove(&id);
                            cx.notify();
                        });
                        return; // the row already shows Cancelled
                    }
                    Err(TryRecvError::Empty) => {}
                    // Queue dropped (fresh connect): stop quietly and keep
                    // the `.part` file so a retry resumes where this stopped.
                    Err(TryRecvError::Disconnected) => return,
                }
                if paused {
                    cx.background_executor().timer(download::PAUSE_POLL).await;
                    continue;
                }
                if prepared.offset >= size {
                    let event = match cx
                        .background_executor()
                        .spawn(async move { download::finish(prepared.part, prepared.dest) })
                        .await
                    {
                        Ok(()) => TransferEvent::Complete,
                        Err(message) => TransferEvent::Fail { message },
                    };
                    let _ = this.update(cx, |ws, cx| {
                        ws.apply_runner_report(id, event, cx);
                    });
                    return;
                }
                let want = download::CHUNK_BYTES.min(size - prepared.offset);
                let step = {
                    let backend = device.clone();
                    let path = job.remote_path.clone();
                    let token = token.clone();
                    let part = prepared.part.clone();
                    let offset = prepared.offset;
                    cx.background_executor()
                        .spawn(async move {
                            download::chunk(backend, token, path, part, offset, want).await
                        })
                        .await
                };
                match step {
                    Ok(0) => {
                        let message =
                            format!("download ended early: {} of {size} bytes", prepared.offset);
                        let _ = this.update(cx, |ws, cx| {
                            ws.apply_runner_report(id, TransferEvent::Fail { message }, cx);
                        });
                        return;
                    }
                    Ok(appended) => {
                        prepared.offset += appended;
                        let now = Instant::now();
                        let elapsed = now.duration_since(window_at).as_millis().max(1) as u64;
                        let speed = (prepared.offset - window_bytes) * 1000 / elapsed;
                        window_bytes = prepared.offset;
                        window_at = now;
                        let event = TransferEvent::Progress {
                            delta_bytes: appended,
                            speed_bytes_per_sec: speed,
                        };
                        let _ = this.update(cx, |ws, cx| {
                            ws.apply_runner_report(id, event, cx);
                        });
                    }
                    Err(message) => {
                        let _ = this.update(cx, |ws, cx| {
                            ws.apply_runner_report(id, TransferEvent::Fail { message }, cx);
                        });
                        return;
                    }
                }
            }
        })
        .detach();
    }

    /// Applies a runner report (download or upload) to the transfer queue on
    /// the UI thread and forgets the control channel once the job is
    /// terminal.
    fn apply_runner_report(&mut self, id: u64, event: TransferEvent, cx: &mut Context<Self>) {
        if !matches!(event, TransferEvent::Progress { .. }) {
            self.download_controls.remove(&id);
            self.upload_controls.remove(&id);
        }
        if let TransferEvent::Fail { message } = &event {
            self.state.error = Some(format!("Transfer failed: {message}"));
        }
        let outcome = self
            .state
            .transfers
            .iter_mut()
            .find(|t| t.id == id)
            .map(|t| apply(t, event));
        if let Some(Err(e)) = outcome {
            self.state.error = Some(e.to_string());
        }
        cx.notify();
    }

    /// §7.3 upload: opens the platform file picker, then enqueues one
    /// chunked upload per picked file into the current folder.
    pub fn pick_uploads(&mut self, cx: &mut Context<Self>) {
        if !self.state.live {
            self.state.error = Some("Not connected — pair a phone first".into());
            cx.notify();
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Choose files to upload".into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = match receiver.await {
                Ok(Ok(Some(paths))) => paths,
                _ => return, // cancelled or picker failure
            };
            _ = this.update(cx, |ws, cx| ws.enqueue_uploads(picked, cx));
        })
        .detach();
    }

    /// Shared drop handler: uploads dropped files into `folder`, or `cwd` when
    /// `None`. Errors when no phone is connected.
    pub fn drop_upload(
        &mut self,
        folder: Option<String>,
        paths: &gpui_kit::ExternalPaths,
        cx: &mut Context<Self>,
    ) {
        if !self.state.live {
            self.state.error = Some("Not connected — pair a phone first".into());
            cx.notify();
            return;
        }
        let files: Vec<std::path::PathBuf> = paths.0.iter().cloned().collect();
        match folder {
            Some(f) => self.enqueue_uploads_to(f, files, cx),
            None => self.enqueue_uploads(files, cx),
        }
    }

    /// Creates a queued transfer per local file and starts its runner.
    pub fn enqueue_uploads(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        let folder = self.state.cwd.clone();
        self.enqueue_uploads_to(folder, paths, cx);
    }

    /// Uploads into an explicit remote folder (a dropped-on folder row, or the
    /// opened folder). Virtual quick-filter views never change `cwd`, so the
    /// opened real folder is always the target.
    pub fn enqueue_uploads_to(
        &mut self,
        folder: String,
        paths: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let folder = folder.trim_end_matches('/').to_string();
        for path in paths {
            let size = match upload::probe(&path) {
                Ok(size) => size,
                Err(message) => {
                    self.state.error = Some(message);
                    continue;
                }
            };
            let Some(name) = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            let id = self
                .state
                .transfers
                .iter()
                .map(|t| t.id)
                .max()
                .unwrap_or(100)
                + 1;
            let mut transfer = new_transfer(
                id,
                name.clone(),
                TransferDirection::Upload,
                size.max(1),
                android18_core::fs::paths::display_path(&format!("{folder}/{name}")),
                true,
                ui::NOW_MS,
            );
            let _ = apply(&mut transfer, TransferEvent::Start);
            self.state.transfers.insert(0, transfer);
            self.state.transfers_open = true;
            self.spawn_upload(
                UploadJob {
                    id,
                    local: path,
                    remote_folder: folder.clone(),
                    name,
                    size,
                },
                cx,
            );
        }
        cx.notify();
    }

    /// Runs a live upload: reads the local file in [`upload::CHUNK_BYTES`]
    /// slices on the background executor, appends each slice via
    /// `upload_chunk`, and honors pause/resume/cancel between chunks.
    fn spawn_upload(&mut self, job: UploadJob, cx: &mut Context<Self>) {
        let (sender, receiver) = mpsc::channel::<upload::UploadCommand>();
        self.upload_controls.insert(job.id, sender);
        let device = self.state.device.clone();
        let token = self.state.token();
        let id = job.id;
        let total = job.size;
        let remote = job.remote_path();
        cx.spawn(async move |this, cx| {
            // Empty files still need one zero-byte chunk so the phone
            // creates them; `created` defers completion until it happened.
            let mut created = total > 0;
            let mut sent = 0u64;
            let mut paused = false;
            let mut window_bytes = 0u64;
            let mut window_at = Instant::now();
            loop {
                match receiver.try_recv() {
                    Ok(upload::UploadCommand::Pause) => paused = true,
                    Ok(upload::UploadCommand::Resume) => paused = false,
                    Ok(upload::UploadCommand::Cancel) => {
                        let _ = this.update(cx, |ws, cx| {
                            ws.apply_runner_report(id, TransferEvent::Cancel, cx);
                        });
                        return;
                    }
                    Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => {}
                }
                if paused {
                    cx.background_executor().timer(download::PAUSE_POLL).await;
                    continue;
                }
                if sent >= total && created {
                    let _ = this.update(cx, |ws, cx| {
                        ws.apply_runner_report(id, TransferEvent::Complete, cx);
                        ws.refresh_list(cx);
                        ws.refresh_storage(cx);
                    });
                    return;
                }
                let want = if total == 0 {
                    0
                } else {
                    upload::CHUNK_BYTES.min((total - sent) as usize)
                };
                let read_job = job.clone();
                let read_at = sent;
                let bytes = match cx
                    .background_executor()
                    .spawn(async move { upload::read_chunk(&read_job, read_at, want) })
                    .await
                {
                    Ok(bytes) => bytes,
                    Err(message) => {
                        let _ = this.update(cx, |ws, cx| {
                            ws.apply_runner_report(id, TransferEvent::Fail { message }, cx);
                        });
                        return;
                    }
                };
                if want > 0 && bytes.is_empty() {
                    let _ = this.update(cx, |ws, cx| {
                        ws.apply_runner_report(
                            id,
                            TransferEvent::Fail {
                                message: "source file ended early".into(),
                            },
                            cx,
                        );
                    });
                    return;
                }
                let chunk = bytes.clone();
                let send_device = device.clone();
                let send_remote = remote.clone();
                let send_token = token.clone();
                let send_offset = sent;
                let outcome = cx
                    .background_executor()
                    .spawn(async move {
                        send_device
                            .upload_chunk(&send_remote, send_offset, chunk, &send_token)
                            .await
                    })
                    .await;
                match outcome {
                    Ok(()) => {
                        sent += bytes.len() as u64;
                        created = true;
                        window_bytes += bytes.len() as u64;
                        let now = Instant::now();
                        if now - window_at >= Duration::from_millis(250) {
                            let speed =
                                (window_bytes as f64 / (now - window_at).as_secs_f64()) as u64;
                            let delta = window_bytes;
                            window_bytes = 0;
                            window_at = now;
                            let _ = this.update(cx, |ws, cx| {
                                ws.apply_runner_report(
                                    id,
                                    TransferEvent::Progress {
                                        delta_bytes: delta,
                                        speed_bytes_per_sec: speed,
                                    },
                                    cx,
                                );
                            });
                        }
                    }
                    Err(e) => {
                        let _ = this.update(cx, |ws, cx| {
                            ws.apply_runner_report(
                                id,
                                TransferEvent::Fail {
                                    message: e.to_string(),
                                },
                                cx,
                            );
                        });
                        return;
                    }
                }
            }
        })
        .detach();
    }
    /// Runs one mutating call in the background, then refreshes the listing.
    fn run_mutation(&mut self, path: String, mutation: Mutation, cx: &mut Context<Self>) {
        let device = self.state.device.clone();
        let token = self.state.token();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match mutation {
                        Mutation::MkDir => device.mkdir(&path, &token).await,
                        Mutation::Mv(to) => device.mv(&path, &to, &token).await,
                    }
                })
                .await;
            _ = this.update(cx, |ws, cx| {
                match result {
                    Ok(()) => ws.state.error = None,
                    Err(e) => ws.state.error = Some(e.to_string()),
                }
                ws.state.selected = None;
                ws.state.selected_preview = None;
                ws.state.selected_preview_image = None;
                ws.refresh_list(cx);
                ws.refresh_storage(cx);
            });
        })
        .detach();
    }

    /// Applies a lifecycle event to a queued transfer through the engine,
    /// forwarding pause/resume/cancel to any live runner.
    /// §12 "Clear finished": drops completed / cancelled / errored rows
    /// from the queue; live and paused transfers stay.
    pub fn clear_finished_transfers(&mut self, cx: &mut Context<Self>) {
        self.state.clear_finished_transfers();
        cx.notify();
    }

    pub fn transfer_event(&mut self, id: u64, event: TransferEvent, cx: &mut Context<Self>) {
        let download_command = match &event {
            TransferEvent::Pause => Some(DownloadCommand::Pause),
            TransferEvent::Resume => Some(DownloadCommand::Resume),
            TransferEvent::Cancel => Some(DownloadCommand::Cancel),
            _ => None,
        };
        let upload_command = match &event {
            TransferEvent::Pause => Some(upload::UploadCommand::Pause),
            TransferEvent::Resume => Some(upload::UploadCommand::Resume),
            TransferEvent::Cancel => Some(upload::UploadCommand::Cancel),
            _ => None,
        };
        if let Some(command) = download_command
            && let Some(sender) = self.download_controls.get(&id)
        {
            let _ = sender.send(command);
        }
        if let Some(command) = upload_command
            && let Some(sender) = self.upload_controls.get(&id)
        {
            let _ = sender.send(command);
        }
        let outcome = self
            .state
            .transfers
            .iter_mut()
            .find(|t| t.id == id)
            .map(|t| apply(t, event));
        match outcome {
            Some(Err(e)) => self.state.error = Some(e.to_string()),
            _ => self.state.error = None,
        }
        cx.notify();
    }

    /// Shows a bottom-center toast (§14.8) that self-dismisses after 2.5 s.
    pub fn show_toast(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.state.toast_seq += 1;
        let seq = self.state.toast_seq;
        self.state.toast = Some(Toast {
            id: seq,
            message: message.into(),
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(2500))
                .await;
            _ = this.update(cx, |ws, cx| {
                if ws.state.toast.as_ref().is_some_and(|t| t.id == seq) {
                    ws.state.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// Name the phone shows on its allow/deny pairing prompt.
fn desktop_name() -> String {
    std::env::var("USER")
        .ok()
        .map(|user| user.trim().to_string())
        .filter(|user| !user.is_empty())
        .map(|user| format!("{user}'s Mac"))
        .unwrap_or_else(|| "Android18 Desktop".into())
}

/// Which mutating device call a background task should run.
enum Mutation {
    MkDir,
    /// Rename/move to the full `to` path.
    Mv(String),
}

impl gpui_kit::Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.state.active_transfers();
        if active != self.dock_badge {
            self.dock_badge = active;
            let label = (active > 0).then(|| active.to_string());
            crate::platform::set_dock_badge(label.as_deref());
        }
        // Re-adopt focus whenever nothing else holds it (e.g. a modal input
        // just unmounted). Deferred so focus moves land outside the render
        // pass, and re-checked inside in case an element grabbed it since.
        if window.focused(cx).is_none() {
            let root_focus = self.root_focus.clone();
            window.defer(cx, move |window, cx| {
                if window.focused(cx).is_none() {
                    window.focus(&root_focus, cx);
                }
            });
        }
        let center: AnyElement = match self.state.view {
            CenterView::Browser => ui::browser::render(self, window, cx),
            CenterView::Dashboard => ui::dashboard::render(self, cx),
        };
        // The onboarding gate replaces the whole shell while no phone is
        // live (§15); overlays below it stay reachable either way.
        let live = self.state.live;
        div()
            .id("workspace")
            .track_focus(&self.root_focus)
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(ui::theme::WHITE)
            .text_color(ui::theme::BLACK)
            .font_family(ui::theme::FONT_SANS)
            .text_size(px(13.))
            .on_action(cx.listener(|this, _: &NavigateBack, _w, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _: &NavigateForward, _w, cx| this.go_forward(cx)))
            .on_action(cx.listener(|this, _: &NavigateUp, _w, cx| this.navigate_up(cx)))
            .on_action(cx.listener(|this, _: &Refresh, _w, cx| this.refresh_list(cx)))
            .on_action(cx.listener(|this, _: &ToggleViewMode, _w, cx| this.toggle_view_mode(cx)))
            .on_action(cx.listener(|this, _: &CycleSort, _w, cx| this.cycle_sort(cx)))
            // §7.3 context-menu actions: the popup dispatches these; the
            // right-click selects the pressed entry before the menu opens.
            .on_action(cx.listener(|this, _: &OpenSelected, _w, cx| {
                if let Some(entry) = this.state.selected_entry().cloned() {
                    this.open_entry(entry.path, entry.dir, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CopySelectedPath, _w, cx| {
                if let Some(path) = this.state.selected.clone() {
                    cx.write_to_clipboard(ClipboardItem::new_string(path));
                    this.show_toast("Path copied to clipboard", cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CopyCurrentPath, _w, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(this.state.cwd.clone()));
                this.show_toast("Path copied to clipboard", cx);
            }))
            .on_action(cx.listener(|this, _: &DownloadSelected, _w, cx| this.download_selected(cx)))
            .on_action(cx.listener(|this, _: &DeleteSelected, _w, cx| this.delete_selected(cx)))
            .on_action(
                cx.listener(|this, _: &RenameSelected, window, cx| this.open_rename(window, cx)),
            )
            .on_action(cx.listener(|this, _: &CopySelected, _w, cx| this.copy_selected(cx)))
            .on_action(cx.listener(|this, _: &CutSelected, _w, cx| this.cut_selected(cx)))
            .on_action(cx.listener(|this, _: &PasteIntoFolder, _w, cx| this.paste_into_folder(cx)))
            .on_action(cx.listener(|this, _: &TogglePin, _w, cx| this.toggle_pin_selected(cx)))
            .on_action(
                cx.listener(|this, _: &TagBlue, _w, cx| {
                    this.tag_selected(Some(ColorTag::Blue), cx)
                }),
            )
            .on_action(cx.listener(|this, _: &TagEmerald, _w, cx| {
                this.tag_selected(Some(ColorTag::Emerald), cx)
            }))
            .on_action(cx.listener(|this, _: &TagAmber, _w, cx| {
                this.tag_selected(Some(ColorTag::Amber), cx)
            }))
            .on_action(cx.listener(|this, _: &TagPurple, _w, cx| {
                this.tag_selected(Some(ColorTag::Purple), cx)
            }))
            .on_action(
                cx.listener(|this, _: &TagRose, _w, cx| {
                    this.tag_selected(Some(ColorTag::Rose), cx)
                }),
            )
            .on_action(cx.listener(|this, _: &TagSlate, _w, cx| {
                this.tag_selected(Some(ColorTag::Slate), cx)
            }))
            .on_action(cx.listener(|this, _: &ClearTag, _w, cx| this.tag_selected(None, cx)))
            .on_action(
                cx.listener(|this, _: &NewFolder, window, cx| this.open_new_folder(window, cx)),
            )
            .on_action(cx.listener(|this, _: &UploadFiles, _w, cx| this.pick_uploads(cx)))
            .on_action(cx.listener(|this, _: &ToggleDashboard, _w, cx| this.toggle_center_view(cx)))
            .on_action(
                cx.listener(|this, _: &ToggleSearch, window, cx| this.open_search(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleTerminal, _w, cx| this.toggle_terminal(cx)))
            .on_action(cx.listener(|this, _: &TerminalHistoryPrev, window, cx| {
                this.terminal_history(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &TerminalHistoryNext, window, cx| {
                this.terminal_history(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleTransfers, _w, cx| this.toggle_transfers(cx)))
            .on_action(cx.listener(|this, _: &ToggleServer, _w, cx| this.toggle_server_sheet(cx)))
            .on_action(cx.listener(|this, _: &ToggleInspector, _w, cx| this.toggle_inspector(cx)))
            .on_action(cx.listener(|this, _: &Escape, _w, cx| this.escape(cx)))
            .on_action(cx.listener(|this, _: &About, _w, cx| this.toggle_about(cx)))
            .on_action(cx.listener(|this, _: &Settings, _w, cx| this.open_settings(cx)))
            .on_action(
                cx.listener(|_this: &mut Self, _: &Minimize, window, _cx| window.minimize_window()),
            )
            .on_action(cx.listener(|_this: &mut Self, _: &Zoom, window, _cx| window.zoom_window()))
            .on_action(
                cx.listener(|_this: &mut Self, _: &ToggleFullscreen, window, _cx| {
                    window.toggle_fullscreen()
                }),
            )
            .on_action(cx.listener(|_this: &mut Self, _: &Quit, _w, cx| cx.quit()))
            // §15 gate vs §6 shell: offline the onboarding gate replaces
            // the top bar and content area entirely.
            .when(!live, |el| {
                el.child(ui::onboarding::render(self, window, cx))
            })
            .when(live, |el| {
                el.child(ui::top_bar::render(self, window, cx)).child(
                    h_flex()
                        .id("content")
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .child(ui::folder_tree::render(self, window, cx))
                        .child(center)
                        .when(
                            self.state.inspector_open && self.state.selected_entry().is_some(),
                            |el| el.child(ui::inspector::render(self, cx)),
                        )
                        .when(self.state.terminal_open, |el| {
                            el.child(ui::terminal::overlay(self, cx))
                        })
                        .when(self.state.transfers_open, |el| {
                            el.child(ui::transfers::drawer(self, cx))
                        }),
                )
            })
            .when(self.state.search_open, |el| {
                el.child(ui::search::modal(self, cx))
            })
            .when(self.state.server_open, |el| {
                el.child(ui::server_sheet::modal(self, window, cx))
            })
            .when(self.state.new_folder_open, |el| {
                el.child(ui::modals::new_folder(self, window, cx))
            })
            .when(self.state.rename_open, |el| {
                el.child(ui::modals::rename(self, window, cx))
            })
            .when(self.state.about_open, |el| {
                el.child(ui::modals::about(self, cx))
            })
            .when(self.state.settings_open, |el| {
                el.child(ui::modals::settings(self, cx))
            })
            .when_some(self.state.toast.clone(), |el, toast| {
                el.child(ui::modals::toast(&toast))
            })
            .when_some(self.state.error.clone(), |el, error| {
                el.child(ui::modals::error_banner(&error))
            })
    }
}
