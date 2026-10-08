//! Shared application state (plain data + pure helpers).
//!
//! All mutations happen on the [`crate::workspace::Workspace`] entity; async
//! loaders live there so they can spawn on the background executor and apply
//! results with request-sequence guards.

use std::collections::HashSet;
use std::sync::Arc;

use android18_core::domain::view::sort_entries;
use android18_core::domain::{ColorTag, Device, Entry, SortSpec, TransferItem, ViewMode};
use android18_core::fs::MockDevice;
use android18_core::fs::paths::{STORAGE_ROOT, display_path, parent_of};
use android18_core::port::SharedBackend;
use android18_core::search::SearchResult;
use android18_core::shell::ShellLine;
use android18_core::util::categorize::FileCategory;
use gpui_kit::{Hsla, RenderImage};

use crate::icon;
use crate::theme;

/// Where a device-picker candidate came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSource {
    /// mDNS on the LAN.
    Wlan,
    /// USB via `adb forward`.
    Usb,
}

impl DeviceSource {
    pub fn label(self) -> &'static str {
        match self {
            DeviceSource::Wlan => "Wi-Fi",
            DeviceSource::Usb => "USB",
        }
    }
}

/// One row in the Devices modal (R3 pairing surface).
#[derive(Debug, Clone)]
pub struct DeviceCandidate {
    pub id: String,
    pub name: String,
    pub source: DeviceSource,
    /// `http://ip:port`; empty until a USB tunnel exists.
    pub base_url: String,
    /// A pairing token for this id is already saved.
    pub paired: bool,
    /// USB serial of `adb:<serial>` candidates, used for automatic token
    /// pickup and tunnel rebuilds.
    pub adb_serial: Option<String>,
    /// The phone advertises a 6-char pair code (Wi-Fi rows); Connect asks
    /// for it. Rows without one rely on the phone's allow prompt.
    pub code_pairing: bool,
}

/// Which surface fills the browser column (§6): file browser or dashboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CenterView {
    Browser,
    Dashboard,
}

/// One breadcrumb hop: display label plus the path it navigates to.
pub struct Crumb {
    pub label: String,
    pub path: String,
}

/// Bottom-center toast (§14.8); auto-dismissed by the workspace.
#[derive(Clone)]
pub struct Toast {
    pub id: u64,
    pub message: String,
}

/// What pasting will do with the staged paths (§7.1 cut/copy/paste).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardMode {
    /// Paste duplicates the files; the originals stay put.
    Copy,
    /// Paste moves the files (the staged originals disappear).
    Cut,
}

impl ClipboardMode {
    /// Toolbar chip verb: "2 items copied" / "3 items cut".
    pub fn verb(self) -> &'static str {
        match self {
            ClipboardMode::Copy => "copied",
            ClipboardMode::Cut => "cut",
        }
    }
}

/// Files staged for the next paste into a folder.
#[derive(Debug, Clone)]
pub struct Clipboard {
    pub mode: ClipboardMode,
    pub paths: Vec<String>,
}

impl Clipboard {
    /// Toolbar chip caption, e.g. "3 items cut".
    pub fn chip_label(&self) -> String {
        let noun = if self.paths.len() == 1 {
            "item"
        } else {
            "items"
        };
        format!("{} {} {}", self.paths.len(), noun, self.mode.verb())
    }
}

/// A §8.2 quick-access filter: one of the file-type categories, or a color
/// tag. An active filter replaces the browser listing with a virtual folder
/// built client-side from the last full-tree snapshot (no device calls);
/// navigating into any folder clears it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum QuickFilter {
    /// Any file under `~/Download/`.
    Downloads,
    Images,
    Videos,
    Audio,
    /// Text-ish types: documents, notes, spreadsheets, code/config.
    Documents,
    /// Everything tagged with one color.
    Tag(ColorTag),
}

impl QuickFilter {
    /// The file-type rows rendered in the sidebar's Quick access section.
    pub const TYPES: [QuickFilter; 5] = [
        QuickFilter::Downloads,
        QuickFilter::Images,
        QuickFilter::Videos,
        QuickFilter::Audio,
        QuickFilter::Documents,
    ];

    /// Sidebar / breadcrumb label.
    pub fn label(&self) -> String {
        match self {
            QuickFilter::Downloads => "Downloads".into(),
            QuickFilter::Images => "Images".into(),
            QuickFilter::Videos => "Videos".into(),
            QuickFilter::Audio => "Audio".into(),
            QuickFilter::Documents => "Documents".into(),
            QuickFilter::Tag(tag) => format!("Tagged {}", tag.as_str()),
        }
    }

    /// Whether an entry from the tree snapshot belongs to this filter.
    /// Type filters keep files only; tag filters keep tagged folders too
    /// (they navigate on double-click).
    pub fn matches(&self, entry: &Entry) -> bool {
        match self {
            QuickFilter::Downloads => {
                !entry.dir && entry.path.starts_with(&format!("{STORAGE_ROOT}/Download/"))
            }
            QuickFilter::Images => {
                !entry.dir
                    && matches!(
                        FileCategory::of(entry),
                        FileCategory::Image | FileCategory::BinaryImage
                    )
            }
            QuickFilter::Videos => {
                !entry.dir && matches!(FileCategory::of(entry), FileCategory::Video)
            }
            QuickFilter::Audio => {
                !entry.dir && matches!(FileCategory::of(entry), FileCategory::Audio)
            }
            QuickFilter::Documents => {
                !entry.dir
                    && matches!(
                        FileCategory::of(entry),
                        FileCategory::Document
                            | FileCategory::MarkdownNote
                            | FileCategory::Spreadsheet
                            | FileCategory::CodeConfig
                    )
            }
            QuickFilter::Tag(tag) => entry.color_tag == Some(*tag),
        }
    }
}

/// One §2.5 storage-distribution bucket for the dashboard legend. Buckets
/// rooted at a real folder are clickable: the dashboard navigates there.
pub struct CategorySlice {
    pub label: &'static str,
    pub bytes: u64,
    /// Number of files counted into this bucket.
    pub count: usize,
    pub color: Hsla,
    pub glyph: &'static str,
    /// Folder the bucket's card navigates to (every bucket carries one in
    /// the prototype's volume model, System & Cache included).
    pub folder_path: Option<String>,
}

/// §10 recent-activity tabs, mirroring the prototype's All / Files / Folders
/// segmented control (dir-ness, not category buckets).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RecentFilter {
    #[default]
    All,
    Files,
    Folders,
}

impl RecentFilter {
    pub const ALL: [RecentFilter; 3] = [
        RecentFilter::All,
        RecentFilter::Files,
        RecentFilter::Folders,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RecentFilter::All => "All",
            RecentFilter::Files => "Files",
            RecentFilter::Folders => "Folders",
        }
    }

    /// Whether `entry` belongs to this tab.
    pub fn matches(self, entry: &Entry) -> bool {
        match self {
            RecentFilter::All => true,
            RecentFilter::Files => !entry.dir,
            RecentFilter::Folders => entry.dir,
        }
    }
}

/// Connection banner state: a live phone or the offline placeholder while
/// reconnecting / waiting to pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStatus {
    Live,
    Offline,
}

/// QR-pairing session info rendered by the onboarding gate's QR card.
#[derive(Debug, Clone)]
pub struct PairingSession {
    /// `ip:port` the QR encodes.
    pub endpoint: String,
    /// 6-char one-time code shown under the QR (e.g. `0X1D8C`).
    pub code: String,
}

/// Consecutive failed heartbeats before a live phone is declared gone.
pub const HEARTBEAT_MISSES: u32 = 3;

/// Whether a failed heartbeat means the phone is gone: a rejected token
/// at once, anything else after [`HEARTBEAT_MISSES`] failures in a row.
pub fn heartbeat_lost(error: &android18_core::domain::DeviceError, misses: u32) -> bool {
    matches!(error, android18_core::domain::DeviceError::Unauthorized) || misses >= HEARTBEAT_MISSES
}

pub struct AppState {
    /// Active backend — an empty placeholder until a phone is paired.
    pub device: SharedBackend,
    pub device_info: Device,
    /// True once a live `HttpDevice` replaced the placeholder.
    pub live: bool,
    /// Live pairing endpoint + code while the onboarding gate's QR is up.
    pub pairing_session: Option<PairingSession>,
    /// One-line status under the onboarding gate's QR.
    pub pairing_status: String,
    /// One-line scan/connect status for the gate's Discovered devices.
    pub device_scan: String,
    /// mDNS + adb candidates listed on the onboarding gate.
    pub candidates: Vec<DeviceCandidate>,
    /// The gate's "Advanced: connect by address" section is expanded.
    pub pairing_advanced: bool,
    /// Candidate whose row is showing the inline pair-code input.
    pub code_target: Option<String>,
    /// A connect/approval is in flight; background scans must not
    /// overwrite its status line.
    pub connecting: bool,
    /// USB serials already sent an allow prompt in this offline stretch.
    pub usb_prompted: HashSet<String>,
    pub view: CenterView,
    pub cwd: String,
    /// Back/forward history (§7.2); the index points at the current entry.
    pub history: Vec<String>,
    pub history_index: usize,
    /// Listing of the current directory (unfiltered).
    pub entries: Vec<Entry>,
    /// Full-tree snapshot from the last `walk` (tree, dashboard, search).
    pub all_entries: Vec<Entry>,
    pub all_folders: Vec<Entry>,
    pub expanded: HashSet<String>,
    pub selected: Option<String>,
    /// Multi-select extension of `selected` (which stays the anchor /
    /// inspector focus). Empty = single-select mode.
    pub selected_set: HashSet<String>,
    pub selected_preview: Option<String>,
    /// Decoded preview image for the selection (media categories, live
    /// devices only); cleared alongside `selected_preview`.
    pub selected_preview_image: Option<Arc<RenderImage>>,
    /// Path of the browser row under the pointer; drives the hover-only
    /// Move To / Download row actions.
    pub hovered_row: Option<String>,
    /// §8.2 quick-access filter replacing the browser listing when active.
    pub quick_filter: Option<QuickFilter>,
    pub inspector_open: bool,
    pub view_mode: ViewMode,
    pub sort: SortSpec,
    /// §10 recent-activity filter (dashboard chips).
    pub recent_filter: RecentFilter,
    pub error: Option<String>,
    pub search: Option<SearchResult>,
    /// A ⌘K query is in flight (AI calls can take several seconds).
    pub search_loading: bool,
    pub transfers: Vec<TransferItem>,
    /// Terminal transcript (styled lines from the shell session).
    pub transcript: Vec<ShellLine>,
    /// Used bytes on the volume: the phone's own figure (`StatFs`), or the
    /// visible-file sum when that is larger / unavailable.
    pub storage_used: u64,
    /// Sum of the file sizes in the tree snapshot (what we can actually see).
    pub storage_files_bytes: u64,
    pub terminal_open: bool,
    pub terminal_maximized: bool,
    pub transfers_open: bool,
    pub search_open: bool,
    /// Server sheet (§15): live-device identity, security, request log.
    /// Only meaningful while `live`; pairing lives on the gate instead.
    pub server_open: bool,
    pub new_folder_open: bool,
    /// R4 rename modal.
    pub rename_open: bool,
    /// Paths staged by cut/copy, pasted into a folder later (§7.1).
    pub clipboard: Option<Clipboard>,
    /// Native-menu About modal (§14 overlay family).
    pub about_open: bool,
    /// Settings modal (⌘,): default view, theme, connection/token.
    pub settings_open: bool,
    pub toast: Option<Toast>,
    /// Monotonic tokens so stale async responses never overwrite newer state.
    pub(crate) list_seq: u64,
    pub(crate) search_seq: u64,
    pub(crate) toast_seq: u64,
    /// Guards device scans/connects (R3 pairing flows).
    pub(crate) connect_seq: u64,
    /// Guards the device-scan loop independently of connects, so the
    /// periodic refresh never cancels an in-flight approval.
    pub(crate) scan_seq: u64,
    /// Guards `refresh_storage` so a late walk cannot repopulate the tree
    /// after the phone went offline.
    pub(crate) storage_seq: u64,
    /// Guards the live-connection heartbeat loop.
    pub(crate) heartbeat_seq: u64,
    /// Guards pairing sessions: the gate leaving (a phone going live) or
    /// reopening fresh invalidates the previous poll loop.
    pub(crate) pairing_seq: u64,
}
/// Aggregate facts for the multi-select inspector card (§9).
#[derive(Debug, Default)]
pub struct SelectionSummary {
    pub count: usize,
    pub folders: usize,
    pub files: usize,
    pub bytes: u64,
}

impl AppState {
    /// Drops every piece of phone-derived data and swaps in the offline
    /// placeholder: listings, tree, dashboard, selection, clipboard,
    /// inspector, history and the connection fields. In-flight loads are
    /// cancelled by bumping their sequence guards. Transfers stay.
    pub fn clear_phone_data(&mut self) {
        self.list_seq += 1;
        self.storage_seq += 1;
        self.connect_seq += 1;
        self.heartbeat_seq += 1;
        self.connecting = false;
        self.device = Self::empty_backend(crate::ui::NOW_MS);
        self.device_info = Self::offline_device_info();
        self.live = false;
        self.candidates.clear();
        self.device_scan.clear();
        self.code_target = None;
        self.entries.clear();
        self.all_entries.clear();
        self.all_folders.clear();
        self.storage_used = 0;
        self.storage_files_bytes = 0;
        self.expanded = HashSet::from([STORAGE_ROOT.to_string()]);
        self.selected = None;
        self.selected_set.clear();
        self.selected_preview = None;
        self.selected_preview_image = None;
        self.clipboard = None;
        self.inspector_open = false;
        self.view = CenterView::Browser;
        self.quick_filter = None;
        self.search = None;
        self.error = None;
        self.cwd = STORAGE_ROOT.to_string();
        self.history = vec![STORAGE_ROOT.to_string()];
        self.history_index = 0;
    }

    /// Identity shown while no phone is paired (the empty placeholder's
    /// `device_info`).
    pub fn offline_device_info() -> Device {
        let mut info = Device::new("", "");
        info.name = "No device".into();
        info.model = String::new();
        info.status = android18_core::domain::DeviceStatus::Found;
        info
    }

    /// The bare-root placeholder used while no phone is connected.
    pub fn empty_backend(now_ms: i64) -> SharedBackend {
        Arc::new(MockDevice::empty(now_ms))
    }

    pub fn new(now_ms: i64) -> Self {
        Self {
            device: Self::empty_backend(now_ms),
            device_info: Self::offline_device_info(),
            live: false,
            pairing_session: None,
            pairing_status: String::new(),
            device_scan: String::new(),
            candidates: Vec::new(),
            pairing_advanced: false,
            code_target: None,
            connecting: false,
            usb_prompted: HashSet::new(),
            view: CenterView::Browser,
            cwd: STORAGE_ROOT.to_string(),
            history: vec![STORAGE_ROOT.to_string()],
            history_index: 0,
            entries: Vec::new(),
            all_entries: Vec::new(),
            all_folders: Vec::new(),
            expanded: HashSet::from([STORAGE_ROOT.to_string()]),
            selected: None,
            selected_set: HashSet::new(),
            selected_preview: None,
            selected_preview_image: None,
            hovered_row: None,
            quick_filter: None,
            inspector_open: false,
            view_mode: ViewMode::Table,
            sort: SortSpec::default(),
            recent_filter: RecentFilter::default(),
            error: None,
            search: None,
            transfers: Vec::new(),
            transcript: Vec::new(),
            storage_used: 0,
            storage_files_bytes: 0,
            terminal_open: false,
            terminal_maximized: false,
            transfers_open: false,
            search_open: false,
            server_open: false,
            new_folder_open: false,
            rename_open: false,
            clipboard: None,
            about_open: false,
            settings_open: false,
            toast: None,
            list_seq: 0,
            search_seq: 0,
            search_loading: false,
            toast_seq: 0,
            connect_seq: 0,
            scan_seq: 0,
            storage_seq: 0,
            heartbeat_seq: 0,
            pairing_seq: 0,
        }
    }

    /// Auth token for the active backend (pairing token when live).
    pub fn token(&self) -> String {
        self.device_info.token.clone()
    }

    /// Connection status for pills and the browser empty state.
    pub fn link_status(&self) -> LinkStatus {
        if self.live {
            LinkStatus::Live
        } else {
            LinkStatus::Offline
        }
    }

    /// True when `path` participates in the current selection (set mode
    /// wins over the single anchor).
    pub fn is_selected(&self, path: &str) -> bool {
        if self.selected_set.is_empty() {
            self.selected.as_deref() == Some(path)
        } else {
            self.selected_set.contains(path)
        }
    }

    /// Paths the batch toolbar / context menu act on: the multi-select set,
    /// or the single anchor when it is empty.
    pub fn selection_paths(&self) -> Vec<String> {
        if self.selected_set.is_empty() {
            self.selected.clone().into_iter().collect()
        } else {
            self.selected_set.iter().cloned().collect()
        }
    }

    /// Number of entries the batch actions target.
    pub fn selection_count(&self) -> usize {
        if self.selected_set.is_empty() {
            usize::from(self.selected.is_some())
        } else {
            self.selected_set.len()
        }
    }

    /// Clears both selection modes.
    pub fn clear_selection(&mut self) {
        self.selected = None;
        self.selected_set.clear();
    }

    /// Entries the multi-select inspector shows (§9): the picked set in
    /// listing order; single selections collapse to the anchor entry.
    pub fn selection_entries(&self) -> Vec<Entry> {
        if self.selected_set.len() < 2 {
            return self.selected_entry().cloned().into_iter().collect();
        }
        let mut picked: Vec<Entry> = self
            .sorted_entries()
            .into_iter()
            .filter(|e| self.selected_set.contains(&e.path))
            .collect();
        // Entries picked in a virtual (quick-filter) view may not sit in the
        // cwd listing; pull those from the full-tree snapshot.
        let listed: HashSet<String> = picked.iter().map(|e| e.path.clone()).collect();
        for path in &self.selected_set {
            if !listed.contains(path)
                && let Some(entry) = self.all_entries.iter().find(|e| &e.path == path)
            {
                picked.push(entry.clone());
            }
        }
        picked
    }

    /// Aggregate facts over the current selection (§9 multi view).
    pub fn selection_summary(&self) -> SelectionSummary {
        let entries = self.selection_entries();
        let mut summary = SelectionSummary {
            count: entries.len(),
            ..SelectionSummary::default()
        };
        for entry in &entries {
            if entry.dir {
                summary.folders += 1;
            } else {
                summary.files += 1;
                summary.bytes += entry.size;
            }
        }
        summary
    }

    /// Drops terminal transfers (completed, cancelled, errored) from the
    /// queue, keeping anything still queued, running or paused.
    pub fn clear_finished_transfers(&mut self) {
        self.transfers.retain(|t| !t.status.is_terminal());
    }

    /// Whether the transfers drawer has anything finished to clear.
    pub fn has_finished_transfers(&self) -> bool {
        self.transfers.iter().any(|t| t.status.is_terminal())
    }

    /// Entries for the current directory in the current sort order.
    pub fn sorted_entries(&self) -> Vec<Entry> {
        let mut entries = self.entries.clone();
        sort_entries(&mut entries, &self.sort);
        entries
    }

    /// Sorted entries whose name contains `filter` (case-insensitive).
    pub fn filtered_entries(&self, filter: &str) -> Vec<Entry> {
        let needle = filter.trim().to_lowercase();
        if needle.is_empty() {
            return self.sorted_entries();
        }
        self.sorted_entries()
            .into_iter()
            .filter(|e| e.name.to_lowercase().contains(&needle))
            .collect()
    }

    /// Breadcrumb hops from the storage root (`~`) down to the cwd. An
    /// active quick-access filter is a virtual folder on top of the root:
    /// the trail becomes `~ / <filter>` and stops there.
    pub fn breadcrumb(&self) -> Vec<Crumb> {
        let mut crumbs = vec![Crumb {
            label: "~".into(),
            path: STORAGE_ROOT.to_string(),
        }];
        if let Some(filter) = &self.quick_filter {
            // Clicking the last hop must not navigate anywhere (the filter
            // keeps the cwd underneath); reuse the cwd as a no-op target.
            crumbs.push(Crumb {
                label: filter.label(),
                path: self.cwd.clone(),
            });
            return crumbs;
        }
        let Some(suffix) = self.cwd.strip_prefix(&format!("{STORAGE_ROOT}/")) else {
            return crumbs;
        };
        let mut path = STORAGE_ROOT.to_string();
        for segment in suffix.split('/').filter(|s| !s.is_empty()) {
            path = format!("{path}/{segment}");
            crumbs.push(Crumb {
                label: segment.to_string(),
                path: path.clone(),
            });
        }
        crumbs
    }

    /// Looks a path up in the current listing, then the full-tree snapshot.
    pub fn entry_at(&self, path: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.path == path)
            .or_else(|| self.all_entries.iter().find(|e| e.path == path))
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.selected.as_deref().and_then(|p| self.entry_at(p))
    }

    /// Sub-folders of `path` from the tree snapshot, alphabetical.
    pub fn child_folders(&self, path: &str) -> Vec<&Entry> {
        let mut folders: Vec<&Entry> = self
            .all_folders
            .iter()
            .filter(|e| parent_of(&e.path).is_some_and(|p| p == path))
            .collect();
        folders.sort_by_key(|a| a.name.to_lowercase());
        folders
    }

    /// §8.2 quick access: the browser listing for an active filter — the
    /// matching entries from the last full-tree snapshot, in the current
    /// sort order (a pure client-side virtual folder).
    pub fn quick_filter_entries(&self, filter: &QuickFilter) -> Vec<Entry> {
        let mut entries: Vec<Entry> = self
            .all_entries
            .iter()
            .filter(|e| filter.matches(e))
            .cloned()
            .collect();
        sort_entries(&mut entries, &self.sort);
        entries
    }

    /// Sidebar count badge for one quick-access row.
    pub fn quick_filter_count(&self, filter: &QuickFilter) -> usize {
        self.all_entries
            .iter()
            .filter(|e| filter.matches(e))
            .count()
    }

    /// §8.2 Tags section: colors in use across the tree snapshot, in the
    /// fixed menu order, with counts. Empty result → section hidden.
    pub fn tag_summary(&self) -> Vec<(ColorTag, usize)> {
        const TAGS: [ColorTag; 6] = [
            ColorTag::Blue,
            ColorTag::Emerald,
            ColorTag::Amber,
            ColorTag::Purple,
            ColorTag::Rose,
            ColorTag::Slate,
        ];
        TAGS.iter()
            .map(|tag| {
                (
                    *tag,
                    self.all_entries
                        .iter()
                        .filter(|e| e.color_tag == Some(*tag))
                        .count(),
                )
            })
            .filter(|(_, count)| *count > 0)
            .collect()
    }

    /// §2.5 storage-distribution buckets. Counts and bytes are real, summed
    /// from the tree snapshot per file category. Space the walk cannot see
    /// (apps, system, `Android/data`, dotfiles) is the difference between the
    /// phone-reported used bytes and the visible files, and lands in
    /// System & Cache so the legend adds up to `storage_used`. Every bucket
    /// card navigates to the folder that holds its content, resolved against
    /// the folders that really exist on the phone.
    pub fn dashboard_categories(&self) -> Vec<CategorySlice> {
        let mut counts = [0usize; 6];
        let mut bytes = [0u64; 6];
        for entry in &self.all_entries {
            if entry.dir {
                continue;
            }
            let bucket = match FileCategory::of(entry) {
                FileCategory::Image => 0,
                FileCategory::Video => 1,
                FileCategory::Audio => 2,
                FileCategory::Document | FileCategory::MarkdownNote | FileCategory::Spreadsheet => {
                    3
                }
                FileCategory::AndroidPackage
                | FileCategory::Archive
                | FileCategory::BinaryImage => 4,
                FileCategory::CodeConfig | FileCategory::File | FileCategory::Folder => 5,
            };
            counts[bucket] += 1;
            bytes[bucket] += entry.size;
        }
        let visible: u64 = bytes.iter().sum();
        bytes[5] += self.storage_used.saturating_sub(visible);
        let target = |candidates: &[&str]| -> Option<String> {
            let full: Vec<String> = candidates
                .iter()
                .map(|c| format!("{STORAGE_ROOT}/{c}"))
                .collect();
            if self.all_folders.is_empty() {
                // Tree not loaded yet — assume the conventional location.
                return full.into_iter().next();
            }
            full.into_iter()
                .find(|p| self.all_folders.iter().any(|f| &f.path == p))
                .or_else(|| Some(STORAGE_ROOT.to_string()))
        };
        vec![
            CategorySlice {
                label: "Images & Photos",
                bytes: bytes[0],
                count: counts[0],
                color: theme::AMBER_500,
                glyph: icon::IMAGE,
                folder_path: target(&["DCIM/Camera", "Pictures", "DCIM"]),
            },
            CategorySlice {
                label: "Videos & Media",
                bytes: bytes[1],
                count: counts[1],
                color: theme::PURPLE_500,
                glyph: icon::FILM_STRIP,
                folder_path: target(&["Movies", "DCIM/Camera", "DCIM"]),
            },
            CategorySlice {
                label: "Audio & Music",
                bytes: bytes[2],
                count: counts[2],
                color: theme::SKY_500,
                glyph: icon::MUSIC_NOTES,
                folder_path: target(&["Music"]),
            },
            CategorySlice {
                label: "Documents & Notes",
                bytes: bytes[3],
                count: counts[3],
                color: theme::EMERALD_600,
                glyph: icon::FILE_TEXT,
                folder_path: target(&["Documents"]),
            },
            CategorySlice {
                label: "APKs & Downloads",
                bytes: bytes[4],
                count: counts[4],
                color: theme::ROSE_500,
                glyph: icon::ANDROID_LOGO,
                folder_path: target(&["Download"]),
            },
            CategorySlice {
                label: "System & Cache",
                bytes: bytes[5],
                count: counts[5],
                color: theme::SLATE_400,
                glyph: icon::CPU,
                folder_path: target(&["Android"]),
            },
        ]
    }

    /// The `n` most recently modified entries in the tree (root excluded),
    /// narrowed to `filter`'s tab and to names/paths containing `query`
    /// (§10 recents tabs + instant search).
    pub fn recents_filtered(&self, filter: RecentFilter, query: &str, n: usize) -> Vec<&Entry> {
        let needle = query.to_lowercase();
        let mut recent: Vec<&Entry> = self
            .all_entries
            .iter()
            .filter(|e| e.path != STORAGE_ROOT && filter.matches(e))
            .filter(|e| {
                needle.is_empty()
                    || e.name.to_lowercase().contains(&needle)
                    || e.path.to_lowercase().contains(&needle)
            })
            .collect();
        recent.sort_by(|a, b| b.mtime.cmp(&a.mtime).then(a.name.cmp(&b.name)));
        recent.truncate(n);
        recent
    }

    /// Pushes `path` onto the history, dropping any forward entries (§7.2).
    pub fn push_history(&mut self, path: &str) {
        if self.history.get(self.history_index) == Some(&path.to_string()) {
            return;
        }
        self.history.truncate(self.history_index + 1);
        self.history.push(path.to_string());
        if self.history.len() > 32 {
            self.history.remove(0);
        }
        self.history_index = self.history.len() - 1;
    }

    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    pub fn active_transfers(&self) -> usize {
        self.transfers
            .iter()
            .filter(|t| t.status.is_active())
            .count()
    }

    pub fn can_go_up(&self) -> bool {
        parent_of(&self.cwd).is_some()
    }

    /// Used-bytes fraction of the device storage, in `0.0..=1.0`.
    pub fn storage_fraction(&self) -> f32 {
        let total = self.device_info.storage_total_bytes.max(1);
        (self.storage_used as f32 / total as f32).clamp(0.0, 1.0)
    }

    pub fn display_cwd(&self) -> String {
        display_path(&self.cwd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use android18_core::domain::{TransferDirection, TransferStatus};
    use android18_core::transfer::{TransferEvent, apply, new_transfer};

    #[test]
    fn clear_phone_data_drops_listings_tree_and_selection() {
        let mut s = AppState::new(0);
        s.live = true;
        s.connecting = true;
        s.storage_used = 42;
        s.inspector_open = true;
        s.view = CenterView::Dashboard;
        s.cwd = format!("{STORAGE_ROOT}/DCIM");
        s.selected_set.insert(format!("{STORAGE_ROOT}/a"));
        s.expanded.insert(format!("{STORAGE_ROOT}/DCIM"));
        s.all_entries.push(entry_stub());
        s.all_folders.push(entry_stub());
        s.entries.push(entry_stub());
        let (list, storage, heartbeat) = (s.list_seq, s.storage_seq, s.heartbeat_seq);
        s.clear_phone_data();
        assert!(!s.live && !s.connecting && !s.inspector_open);
        assert!(s.entries.is_empty() && s.all_entries.is_empty() && s.all_folders.is_empty());
        assert!(s.selected_set.is_empty() && s.clipboard.is_none());
        assert_eq!(s.storage_used, 0);
        assert_eq!(s.expanded.len(), 1);
        assert_eq!(s.view, CenterView::Browser);
        assert_eq!(s.cwd, STORAGE_ROOT);
        assert!(s.list_seq > list && s.storage_seq > storage && s.heartbeat_seq > heartbeat);
    }

    fn entry_stub() -> Entry {
        serde_json::from_value(serde_json::json!({
            "name": "a", "path": format!("{STORAGE_ROOT}/a"), "dir": true,
            "size": 0, "mtime": 0
        }))
        .expect("entry")
    }

    #[test]
    fn heartbeat_drops_on_unauthorized_or_third_miss() {
        use android18_core::domain::DeviceError;
        let offline = DeviceError::Offline("refused".into());
        assert!(!heartbeat_lost(&offline, 1));
        assert!(!heartbeat_lost(&offline, 2));
        assert!(heartbeat_lost(&offline, 3));
        assert!(heartbeat_lost(&DeviceError::Unauthorized, 1));
    }

    #[test]
    fn breadcrumb_splits_segments() {
        let mut s = AppState::new(0);
        s.cwd = format!("{STORAGE_ROOT}/DCIM/Camera");
        let crumbs = s.breadcrumb();
        assert_eq!(crumbs.len(), 3);
        assert_eq!(crumbs[0].label, "~");
        assert_eq!(crumbs[2].label, "Camera");
        assert_eq!(crumbs[2].path, format!("{STORAGE_ROOT}/DCIM/Camera"));
    }

    #[test]
    fn history_drops_forward_and_caps_length() {
        let mut s = AppState::new(0);
        let a = format!("{STORAGE_ROOT}/a");
        let b = format!("{STORAGE_ROOT}/b");
        s.push_history(&a);
        s.push_history(&b);
        assert!(s.can_go_back());
        assert!(!s.can_go_forward());
        s.history_index = 1; // "go back" to a
        s.push_history(&a); // no-op: already current; forward entry kept
        assert_eq!(
            s.history,
            vec![STORAGE_ROOT.to_string(), a.clone(), b.clone()]
        );
        s.push_history(&b); // forward entries dropped from here
        assert_eq!(s.history.len(), 3);
        for i in 0..40 {
            s.push_history(&format!("{STORAGE_ROOT}/d{i}"));
        }
        assert!(s.history.len() <= 33);
        assert_eq!(s.history_index, s.history.len() - 1);
    }

    #[test]
    fn quick_filters_partition_by_type_and_tag() {
        let mut s = AppState::new(0);
        s.all_entries = vec![
            Entry::file(
                &format!("{STORAGE_ROOT}/Download/app.apk"),
                10,
                10,
                None,
                None,
            ),
            Entry::file(
                &format!("{STORAGE_ROOT}/DCIM/IMG_001.jpg"),
                20,
                20,
                None,
                None,
            ),
            Entry::file(&format!("{STORAGE_ROOT}/DCIM/icon.png"), 30, 30, None, None),
            Entry::file(
                &format!("{STORAGE_ROOT}/Movies/clip.mp4"),
                40,
                40,
                None,
                None,
            ),
            Entry::file(
                &format!("{STORAGE_ROOT}/Music/track.mp3"),
                50,
                50,
                None,
                None,
            ),
            Entry::file(
                &format!("{STORAGE_ROOT}/Docs/report.pdf"),
                60,
                60,
                None,
                None,
            ),
            Entry::file(&format!("{STORAGE_ROOT}/Docs/notes.md"), 70, 70, None, None),
            // A jpg outside Download/ is an Image but not a Download.
            Entry::file(
                &format!("{STORAGE_ROOT}/Download/shot.jpg"),
                80,
                80,
                None,
                None,
            ),
            Entry::dir(&format!("{STORAGE_ROOT}/Download"), 90, None, false),
        ];
        // Downloads = files under ~/Download/ regardless of type.
        assert_eq!(s.quick_filter_count(&QuickFilter::Downloads), 2);
        // .jpg and .png both count as images.
        assert_eq!(s.quick_filter_count(&QuickFilter::Images), 3);
        assert_eq!(s.quick_filter_count(&QuickFilter::Videos), 1);
        assert_eq!(s.quick_filter_count(&QuickFilter::Audio), 1);
        // Documents covers pdf + markdown notes.
        assert_eq!(s.quick_filter_count(&QuickFilter::Documents), 2);
        // The virtual listing is exactly the matching entries, sorted.
        let images = s.quick_filter_entries(&QuickFilter::Images);
        assert_eq!(images.len(), 3);
        assert!(images.iter().all(|e| !e.dir));
        // Tag filters match tagged entries — folders included.
        let mut tagged = s.quick_filter_entries(&QuickFilter::Tag(ColorTag::Rose));
        assert!(tagged.is_empty());
        s.all_entries[0].color_tag = Some(ColorTag::Rose);
        s.all_entries[8].color_tag = Some(ColorTag::Rose);
        tagged = s.quick_filter_entries(&QuickFilter::Tag(ColorTag::Rose));
        assert_eq!(tagged.len(), 2);
        assert!(tagged.iter().any(|e| e.dir));
    }

    #[test]
    fn tag_summary_counts_and_hides_empty() {
        let mut s = AppState::new(0);
        assert!(s.tag_summary().is_empty());
        let mut blue = Entry::file(&format!("{STORAGE_ROOT}/a.txt"), 1, 1, None, None);
        blue.color_tag = Some(ColorTag::Blue);
        let mut also_blue = Entry::dir(&format!("{STORAGE_ROOT}/dir"), 2, None, false);
        also_blue.color_tag = Some(ColorTag::Blue);
        let mut amber = Entry::file(&format!("{STORAGE_ROOT}/b.txt"), 3, 3, None, None);
        amber.color_tag = Some(ColorTag::Amber);
        s.all_entries = vec![blue, also_blue, amber];
        assert_eq!(
            s.tag_summary(),
            vec![(ColorTag::Blue, 2), (ColorTag::Amber, 1)]
        );
    }

    #[test]
    fn breadcrumb_shows_active_filter_as_virtual_folder() {
        let mut s = AppState::new(0);
        s.cwd = format!("{STORAGE_ROOT}/DCIM/Camera");
        s.quick_filter = Some(QuickFilter::Images);
        let crumbs = s.breadcrumb();
        assert_eq!(crumbs.len(), 2);
        assert_eq!(crumbs[0].label, "~");
        assert_eq!(crumbs[1].label, "Images");
        // The filter hop never navigates away.
        assert_eq!(crumbs[1].path, s.cwd);
        // Clearing the filter restores the real trail.
        s.quick_filter = None;
        assert_eq!(s.breadcrumb().len(), 3);
    }

    #[test]
    fn dashboard_buckets_cover_all_bytes() {
        let mut s = AppState::new(0);
        s.all_entries = vec![
            Entry::file(&format!("{STORAGE_ROOT}/a.jpg"), 100, 10, None, None),
            Entry::file(&format!("{STORAGE_ROOT}/b.mp3"), 50, 10, None, None),
        ];
        s.storage_used = 1000;
        let buckets = s.dashboard_categories();
        assert_eq!(buckets.len(), 6);
        assert_eq!(buckets[0].bytes, 100);
        assert_eq!(buckets[2].bytes, 50);
        // Unseen space (apps/system) lands in System & Cache.
        assert_eq!(buckets[5].bytes, 850);
        let sum: u64 = buckets.iter().map(|b| b.bytes).sum();
        assert_eq!(sum, s.storage_used);
    }

    #[test]
    fn dashboard_bucket_counts_and_click_targets() {
        let mut s = AppState::new(0);
        s.all_entries = vec![
            Entry::file(
                &format!("{STORAGE_ROOT}/DCIM/Camera/IMG_001.jpg"),
                100,
                10,
                None,
                None,
            ),
            Entry::file(
                &format!("{STORAGE_ROOT}/Music/track.mp3"),
                200,
                20,
                None,
                None,
            ),
            Entry::file(
                &format!("{STORAGE_ROOT}/Docs/report.pdf"),
                300,
                30,
                None,
                None,
            ),
            Entry::file(&format!("{STORAGE_ROOT}/blob.xyz"), 400, 40, None, None),
        ];
        let buckets = s.dashboard_categories();
        // Every file lands in exactly one bucket's count.
        let counts: usize = buckets.iter().map(|b| b.count).sum();
        assert_eq!(counts, 4);
        // Every bucket card navigates somewhere under the storage root,
        // System & Cache included (→ Android in the prototype's model).
        for bucket in &buckets {
            let path = bucket.folder_path.as_deref().expect("click target");
            assert!(path.starts_with(STORAGE_ROOT));
        }
        assert!(
            buckets[5]
                .folder_path
                .as_deref()
                .unwrap()
                .ends_with("/Android")
        );
        assert_eq!(buckets[5].count, 1); // blob.xyz is unclassified
    }

    #[test]
    fn recent_tabs_and_search_partition_entries() {
        let mut s = AppState::new(0);
        s.all_entries = vec![
            Entry::file(
                &format!("{STORAGE_ROOT}/DCIM/Camera/IMG_001.jpg"),
                100,
                10,
                None,
                None,
            ),
            Entry::file(
                &format!("{STORAGE_ROOT}/Music/track.mp3"),
                200,
                20,
                None,
                None,
            ),
            Entry::file(
                &format!("{STORAGE_ROOT}/Docs/report.pdf"),
                300,
                30,
                None,
                None,
            ),
            Entry::file(&format!("{STORAGE_ROOT}/app.apk"), 400, 40, None, None),
            Entry::file(&format!("{STORAGE_ROOT}/blob.xyz"), 500, 50, None, None),
            Entry::dir(&format!("{STORAGE_ROOT}/DCIM"), 60, None, false),
        ];
        // Every tabbed recent list only holds entries its tab accepts…
        for filter in RecentFilter::ALL {
            for entry in s.recents_filtered(filter, "", usize::MAX) {
                assert!(filter.matches(entry), "{filter:?} matched {}", entry.name);
            }
        }
        // …and the tabs split the tree by dir-ness.
        assert_eq!(s.recents_filtered(RecentFilter::Files, "", 10).len(), 5);
        assert_eq!(s.recents_filtered(RecentFilter::Folders, "", 10).len(), 1);
        assert_eq!(s.recents_filtered(RecentFilter::All, "", 10).len(), 6);
        // Instant search matches names and paths, case-insensitively.
        assert_eq!(s.recents_filtered(RecentFilter::All, "img", 10).len(), 1);
        assert_eq!(s.recents_filtered(RecentFilter::All, "CAMERA", 10).len(), 1);
        assert_eq!(
            s.recents_filtered(RecentFilter::Folders, "dcim", 10).len(),
            1
        );
        assert_eq!(s.recents_filtered(RecentFilter::All, "zzz", 10).len(), 0);
        // Newest first.
        let all = s.recents_filtered(RecentFilter::All, "", 10);
        assert_eq!(all[0].name, "DCIM");
    }

    #[test]
    fn active_transfers_counts_only_live() {
        let mut s = AppState::new(0);
        let mut live = new_transfer(1, "a.zip", TransferDirection::Upload, 10, "~", true, 0);
        let _ = apply(&mut live, TransferEvent::Start);
        let mut paused = new_transfer(2, "b.zip", TransferDirection::Download, 10, "~", true, 0);
        let _ = apply(&mut paused, TransferEvent::Start);
        let _ = apply(&mut paused, TransferEvent::Pause);
        s.transfers = vec![live, paused];
        assert_eq!(s.active_transfers(), 1);
        let _ = apply(&mut s.transfers[1], TransferEvent::Resume);
        assert_eq!(s.active_transfers(), 2);
        let _ = apply(&mut s.transfers[1], TransferEvent::Complete);
        assert_eq!(s.transfers[1].status, TransferStatus::Completed);
    }

    #[test]
    fn root_cannot_go_up() {
        assert!(!AppState::new(0).can_go_up());
    }

    #[test]
    fn clipboard_chip_labels() {
        let clip = Clipboard {
            mode: ClipboardMode::Cut,
            paths: vec![
                "/storage/emulated/0/a".into(),
                "/storage/emulated/0/b".into(),
            ],
        };
        assert_eq!(clip.chip_label(), "2 items cut");
        let clip = Clipboard {
            mode: ClipboardMode::Copy,
            paths: vec!["/storage/emulated/0/a".into()],
        };
        assert_eq!(clip.chip_label(), "1 item copied");
    }

    #[test]
    fn clear_finished_transfers_keeps_live() {
        let mut s = AppState::new(0);
        let mut done = new_transfer(1, "a.zip", TransferDirection::Upload, 10, "~", true, 0);
        let _ = apply(&mut done, TransferEvent::Start);
        let _ = apply(&mut done, TransferEvent::Complete);
        let mut paused = new_transfer(2, "b.zip", TransferDirection::Download, 10, "~", true, 0);
        let _ = apply(&mut paused, TransferEvent::Start);
        let _ = apply(&mut paused, TransferEvent::Pause);
        let queued = new_transfer(3, "c.zip", TransferDirection::Download, 10, "~", true, 0);
        s.transfers = vec![done, paused, queued];
        assert!(s.has_finished_transfers());
        s.clear_finished_transfers();
        // Paused is not terminal: it stays until it finishes or is cancelled.
        assert_eq!(s.transfers.len(), 2);
        assert!(s.transfers.iter().all(|t| !t.status.is_terminal()));
        assert!(!s.has_finished_transfers());
    }

    #[test]
    fn selection_summary_counts_multi_select() {
        let mut s = AppState::new(0);
        s.entries = vec![
            Entry::dir(&format!("{STORAGE_ROOT}/DCIM"), 60, None, false),
            Entry::file(&format!("{STORAGE_ROOT}/a.txt"), 100, 10, None, None),
            Entry::file(&format!("{STORAGE_ROOT}/b.txt"), 200, 20, None, None),
        ];
        s.selected_set = HashSet::from([
            format!("{STORAGE_ROOT}/DCIM"),
            format!("{STORAGE_ROOT}/a.txt"),
            format!("{STORAGE_ROOT}/b.txt"),
        ]);
        s.selected = Some(format!("{STORAGE_ROOT}/a.txt"));
        let summary = s.selection_summary();
        assert_eq!(summary.count, 3);
        assert_eq!(summary.folders, 1);
        assert_eq!(summary.files, 2);
        assert_eq!(summary.bytes, 300);
        assert_eq!(s.selection_entries().len(), 3);
    }
}
