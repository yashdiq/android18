//! §7 file browser column: two-row toolbar (§7.1 tool cluster, then the
//! breadcrumb strip) over a table or Finder-style grid listing of the current
//! directory. Rows, tiles and the folder's empty space carry right-click
//! context menus; the right-click selects first, so every menu item acts on
//! the pressed entry.

use crate::ui::A11y as _;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use android18_core::domain::{ColorTag, Entry, SortDirection, SortField, SortSpec, ViewMode};
use android18_core::util::categorize::FileCategory;
use android18_core::util::format::{format_bytes, format_relative_time};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenu, ContextMenuExt as _, PopupMenu};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, Div, ExternalPaths, FontWeight,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement, ScrollHandle, Stateful,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};

use crate::icon::{
    self, ARROW_UP, ARROW_UP_LEFT, CARET_LEFT, CARET_RIGHT, COPY, DEVICE_MOBILE, DOWNLOAD_SIMPLE,
    FILE_TEXT, FILM_STRIP, FOLDER_OPEN, FUNNEL_SIMPLE, IMAGE, LIST, MUSIC_NOTES, PLUS,
    SQUARES_FOUR, STAR_FILL, TRASH, UPLOAD_SIMPLE, X,
};
use crate::state::{Clipboard, ClipboardMode, Crumb, LinkStatus, QuickFilter};
use crate::ui::{self, NOW_MS, label};
use crate::workspace::{
    ClearTag, CopyCurrentPath, CopySelected, CopySelectedPath, CutSelected, DeleteSelected,
    DownloadSelected, NavigateBack, NavigateForward, NavigateUp, NewFolder, OpenSelected,
    PasteIntoFolder, RenameSelected, TagAmber, TagBlue, TagEmerald, TagPurple, TagRose, TagSlate,
    TogglePin, ToggleViewMode, UploadFiles, Workspace,
};

/// Right-click menu builder (see [`entry_menu`] and [`folder_menu`]).
type MenuBuilder = Box<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

pub fn render(ws: &mut Workspace, _window: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    let crumbs = ws.state.breadcrumb();
    let can_back = ws.state.can_go_back();
    let can_forward = ws.state.can_go_forward();
    let can_up = ws.state.can_go_up();
    let sort_text = sort_label(&ws.state.sort);
    let grid = ws.state.view_mode == ViewMode::Grid;
    let filter = ws.state.quick_filter.clone();
    // Active quick-access filter → virtual folder over the tree snapshot.
    let entries = match &filter {
        Some(f) => ws.state.quick_filter_entries(f),
        None => ws.state.filtered_entries(""),
    };
    let selected = ws.state.selected.clone();
    let selected_set = ws.state.selected_set.clone();
    let selection_count = ws.state.selection_count();
    let hovered = ws.state.hovered_row.clone();
    let item_count = entries.len();

    v_flex()
        .id("browser")
        .flex_1()
        .min_w_0()
        .h_full()
        .bg(ui::theme::WHITE)
        // Dropping files anywhere on the browser column uploads them into
        // the current folder (same queue as the picker flow).
        .drag_over::<ExternalPaths>(|s, _, _, _| {
            s.border_2()
                .border_color(ui::theme::DROP_RING.opacity(0.35))
        })
        .on_drop::<ExternalPaths>(cx.listener(|this, paths: &ExternalPaths, _w, cx| {
            this.drop_upload(None, paths, cx);
        }))
        .child(toolbar(
            cx,
            ToolbarSpec {
                can_back,
                can_forward,
                can_up,
                clipboard: ws.state.clipboard.clone(),
                selection_count,
                grid,
                sort_text,
            },
        ))
        .child(breadcrumb_bar(crumbs.as_slice(), item_count, cx))
        .child(if entries.is_empty() {
            match &filter {
                // §8.2 virtual folder: per-type empty message.
                Some(f) => filter_empty_state(f).into_any_element(),
                None => empty_state(ws.state.link_status()).into_any_element(),
            }
        } else if grid {
            // Lazy-load thumbnails for the visible tiles only; arrivals
            // re-render through a coalesced notify.
            let range = visible_range(
                &ws.grid_scroll,
                GRID_ROW_H,
                grid_columns(&ws.grid_scroll),
                entries.len(),
            );
            want_visible(ws, &entries, range, cx);
            let scroll = ws.grid_scroll.clone();
            grid_view(
                &entries,
                &ws.thumbnails,
                &scroll,
                &selected_set,
                selected.as_deref(),
                hovered.as_deref(),
                cx,
            )
            .into_any_element()
        } else {
            let range = visible_range(&ws.table_scroll, TABLE_ROW_H, 1, entries.len());
            want_visible(ws, &entries, range, cx);
            let scroll = ws.table_scroll.clone();
            table_view(
                &entries,
                &ws.thumbnails,
                &scroll,
                &selected_set,
                selected.as_deref(),
                hovered.as_deref(),
                cx,
            )
            .into_any_element()
        })
        .into_any_element()
}

/// Inputs to [`toolbar`] from the render pass (keeps the arg list flat).
struct ToolbarSpec {
    can_back: bool,
    can_forward: bool,
    can_up: bool,
    clipboard: Option<Clipboard>,
    selection_count: usize,
    grid: bool,
    sort_text: String,
}

/// §7.1 toolbar row: history cluster, folder tools and the selection batch
/// bar on the left; view toggle and sort on the right. The row is clipped to
/// its column so it never paints over the inspector when that panel opens.
fn toolbar(cx: &mut Context<Workspace>, spec: ToolbarSpec) -> Stateful<Div> {
    let ToolbarSpec {
        can_back,
        can_forward,
        can_up,
        clipboard,
        selection_count,
        grid,
        sort_text,
    } = spec;
    h_flex()
        .id("browser-toolbar")
        .h(px(48.))
        .px_3()
        .gap_1p5()
        .items_center()
        .flex_shrink_0()
        .min_w_0()
        .overflow_hidden()
        .border_b_1()
        .border_color(ui::theme::SLATE_200)
        // §7.2 history cluster: one bordered group, per the prototype.
        // `.disabled` also swallows clicks and drops the tab stop.
        .child(
            h_flex()
                .h(px(32.))
                .p_0p5()
                .gap_0p5()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(ui::theme::SLATE_200)
                .bg(ui::theme::SLATE_50)
                .child(
                    ui::icon_button("nav-back", CARET_LEFT, "Back")
                        .tooltip_with_action("Back", &NavigateBack, None)
                        .disabled(!can_back)
                        .on_click(cx.listener(|this, _e, _w, cx| this.go_back(cx))),
                )
                .child(
                    ui::icon_button("nav-forward", CARET_RIGHT, "Forward")
                        .tooltip_with_action("Forward", &NavigateForward, None)
                        .disabled(!can_forward)
                        .on_click(cx.listener(|this, _e, _w, cx| this.go_forward(cx))),
                )
                .child(
                    ui::icon_button("nav-up", ARROW_UP, "Up one level")
                        .tooltip_with_action("Up one level", &NavigateUp, None)
                        .disabled(!can_up)
                        .on_click(cx.listener(|this, _e, _w, cx| this.navigate_up(cx))),
                ),
        )
        .child(div().w(px(1.)).h(px(20.)).bg(ui::theme::SLATE_200))
        .child(
            ui::primary_icon_button("new-folder", PLUS, "New Folder", cx)
                .on_click(cx.listener(|this, _e, window, cx| this.open_new_folder(window, cx))),
        )
        .child(
            ui::outline_icon_button("upload", UPLOAD_SIMPLE, "Upload files", cx)
                .on_click(cx.listener(|this, _e, _w, cx| this.pick_uploads(cx))),
        )
        // §7.1 cut/copy/paste cluster: while paths are staged the toolbar
        // shows the mode chip, a Paste action for the current folder, and a
        // cancel ✕ (Escape works too).
        .when_some(clipboard, |el, clipboard| {
            let (chip_fg, chip_bg) = match clipboard.mode {
                ClipboardMode::Copy => (ui::theme::BLUE_600, ui::theme::BLUE_600.opacity(0.1)),
                ClipboardMode::Cut => (ui::theme::AMBER_600, ui::theme::AMBER_600.opacity(0.12)),
            };
            el.child(ui::chip(clipboard.chip_label(), chip_fg, chip_bg))
                .child(
                    Button::new("toolbar-paste")
                        .custom(
                            ButtonCustomVariant::new(cx)
                                .color(ui::theme::SLATE_900)
                                .foreground(ui::theme::WHITE)
                                .hover(ui::theme::SLATE_800)
                                .active(ui::theme::SLATE_700),
                        )
                        .h(px(28.))
                        .px(px(14.))
                        .rounded(ui::theme::RADIUS_LG)
                        .shadow_2xs()
                        .child(
                            h_flex().gap_1p5().items_center().child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(ui::theme::WHITE)
                                    .child("Paste"),
                            ),
                        )
                        .tooltip_with_action("Paste into this folder", &PasteIntoFolder, None)
                        .on_click(cx.listener(|this, _e, _w, cx| this.paste_into_folder(cx))),
                )
                .child(
                    ui::icon_button("clipboard-cancel", X, "Cancel paste")
                        .on_click(cx.listener(|this, _e, _w, cx| this.cancel_clipboard(cx))),
                )
        })
        // Selection batch bar (multi-select aware; mirrors the prototype's
        // toolbar cluster).
        .when(selection_count > 0, |el| {
            el.child(ui::chip(
                if selection_count == 1 {
                    "1 selected".to_string()
                } else {
                    format!("{selection_count} selected")
                },
                ui::theme::SLATE_600,
                ui::theme::SLATE_100,
            ))
            .child(
                ui::icon_button("batch-download", DOWNLOAD_SIMPLE, "Download selected")
                    .on_click(cx.listener(|this, _e, _w, cx| this.download_selected(cx))),
            )
            .child(
                ui::icon_button("batch-copy", COPY, "Copy selected")
                    .on_click(cx.listener(|this, _e, _w, cx| this.copy_selected(cx))),
            )
            .child(
                ui::icon_button("batch-cut", ARROW_UP_LEFT, "Cut selected")
                    .on_click(cx.listener(|this, _e, _w, cx| this.cut_selected(cx))),
            )
            .child(
                ui::icon_button("batch-delete", TRASH, "Delete selected")
                    .text_color(ui::theme::ROSE_500)
                    .on_click(cx.listener(|this, _e, _w, cx| this.delete_selected(cx))),
            )
            .child(
                ui::icon_button("deselect", X, "Clear selection").on_click(cx.listener(
                    |this, _e, _w, cx| {
                        this.state.selected_set.clear();
                        this.select_entry(None, cx);
                    },
                )),
            )
        })
        .child(div().flex_1().min_w_0())
        .child(view_toggle(grid, cx))
        .child(
            h_flex()
                .id("sort-order")
                .a11y_button("Sort order")
                .flex_shrink_0()
                .h(px(28.))
                .px_2()
                .gap_1()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(ui::theme::SLATE_200)
                .cursor_pointer()
                .hover(|s| s.border_color(ui::theme::SLATE_300))
                .child(icon::icon(FUNNEL_SIMPLE, px(14.), ui::theme::SLATE_400))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(ui::theme::SLATE_600)
                        .child(sort_text),
                )
                .on_click(cx.listener(|this, _e, _w, cx| this.cycle_sort(cx))),
        )
}

/// §7.1 list ↔ grid segmented control; the active segment sits on a white
/// pill over the slate track. ⌘G toggles either way.
fn view_toggle(grid: bool, cx: &mut Context<Workspace>) -> Div {
    h_flex()
        .flex_shrink_0()
        .h(px(32.))
        .p_0p5()
        .gap_0p5()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(ui::theme::SLATE_200)
        .bg(ui::theme::SLATE_100)
        .child(segment_button(
            "view-list",
            LIST,
            "List view",
            !grid,
            cx,
            |this, cx| {
                if this.state.view_mode == ViewMode::Grid {
                    this.toggle_view_mode(cx);
                }
            },
        ))
        .child(segment_button(
            "view-grid",
            SQUARES_FOUR,
            "Grid view",
            grid,
            cx,
            |this, cx| {
                if this.state.view_mode != ViewMode::Grid {
                    this.toggle_view_mode(cx);
                }
            },
        ))
}

/// One segment of [`view_toggle`], mirroring the §6.1 switch styling.
fn segment_button(
    id: &'static str,
    glyph: &str,
    tip: &'static str,
    active: bool,
    cx: &mut Context<Workspace>,
    on_click: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
) -> Button {
    Button::new(id)
        .custom(if active {
            ButtonCustomVariant::new(cx)
                .color(ui::theme::WHITE)
                .hover(ui::theme::WHITE)
                .active(ui::theme::SLATE_200)
        } else {
            ButtonCustomVariant::new(cx)
                .foreground(ui::theme::SLATE_400)
                .hover(ui::theme::SLATE_200.opacity(0.5))
                .active(ui::theme::SLATE_200.opacity(0.7))
        })
        .when(active, |el| el.text_color(ui::theme::BLACK))
        .icon(Icon::empty().path(glyph))
        .with_size(px(17.))
        .size(px(26.))
        .rounded(px(6.))
        .tooltip_with_action(tip, &ToggleViewMode, None)
        .accessibility_label(tip)
        .on_click(cx.listener(move |this, _e, _w, cx| on_click(this, cx)))
}

/// §7.2 breadcrumb strip: trail on the left, item count + copy-path on the
/// right (the prototype's second toolbar row).
fn breadcrumb_bar(
    crumbs: &[Crumb],
    item_count: usize,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    h_flex()
        .id("breadcrumb-bar")
        .h(px(36.))
        .px_3()
        .gap_2()
        .items_center()
        .flex_shrink_0()
        .border_b_1()
        .border_color(ui::theme::SLATE_200)
        .bg(ui::theme::SLATE_50)
        .child(breadcrumbs(crumbs, cx))
        .child(
            div()
                .text_size(px(12.))
                .text_color(ui::theme::SLATE_400)
                .child(format!(
                    "{item_count} item{}",
                    if item_count == 1 { "" } else { "s" }
                )),
        )
        .child(
            ui::icon_button("copy-path", COPY, "Copy path").on_click(cx.listener(
                |this, _e, _w, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(this.state.cwd.clone()));
                    this.show_toast("Path copied to clipboard", cx);
                },
            )),
        )
}

/// §7.2 breadcrumb trail; the last hop renders as a white pill.
fn breadcrumbs(crumbs: &[Crumb], cx: &mut Context<Workspace>) -> Div {
    h_flex()
        .min_w_0()
        .flex_1()
        .gap_1()
        .items_center()
        .px_1()
        .overflow_x_hidden()
        .children(
            crumbs
                .iter()
                .enumerate()
                .map(|(i, crumb)| {
                    let path = crumb.path.clone();
                    let last = i + 1 == crumbs.len();
                    let mut hop =
                        h_flex().items_center().gap_1().min_w_0().child(
                            h_flex()
                                .id(gpui_kit::SharedString::from(format!(
                                    "crumb-{}",
                                    crumb.path
                                )))
                                .a11y_button(format!("Go to {}", crumb.label))
                                .h(px(22.))
                                .px_1p5()
                                .items_center()
                                .rounded_md()
                                .cursor_pointer()
                                .text_size(px(12.))
                                .when(last, |el| {
                                    el.bg(ui::theme::WHITE)
                                        .border_1()
                                        .border_color(ui::theme::SLATE_200)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(ui::theme::BLACK)
                                })
                                .when(!last, |el| {
                                    el.text_color(ui::theme::SLATE_500)
                                        .hover(|s| s.text_color(ui::theme::BLACK))
                                })
                                .child(crumb.label.clone())
                                .on_click(cx.listener(move |this, _e, _w, cx| {
                                    this.navigate(path.clone(), cx)
                                })),
                        );
                    if !last {
                        hop = hop
                            .child(icon::icon(CARET_RIGHT, px(10.), ui::theme::SLATE_300))
                            .flex_shrink_0();
                    }
                    hop
                })
                .collect::<Vec<_>>(),
        )
}

/// §7.3 table listing with a 32px header and 44px rows. The scroll container
/// carries the folder's right-click menu, so the whole empty area below the
/// rows is menu-active; the filler under the last row is a fixed-height
/// breathing gap (it must not stretch, or the gap below the list visibly
/// resizes with the window).
fn is_picked(selected_set: &HashSet<String>, anchor: Option<&str>, path: &str) -> bool {
    if selected_set.is_empty() {
        anchor == Some(path)
    } else {
        selected_set.contains(path)
    }
}

/// Entries the phone can render a thumbnail for (images, video frames, PDFs).
pub(crate) fn thumbable(entry: &Entry) -> bool {
    if entry.dir {
        return false;
    }
    match FileCategory::of(entry) {
        FileCategory::Image | FileCategory::BinaryImage | FileCategory::Video => true,
        _ => entry
            .extension
            .as_deref()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf")),
    }
}

const TABLE_ROW_H: f32 = 44.;
const GRID_ROW_H: f32 = 152.;
/// Rows fetched beyond the viewport on each side.
const OVERSCAN_ROWS: usize = 6;

/// Number of tiles per grid row, from the tracked viewport width.
fn grid_columns(scroll: &ScrollHandle) -> usize {
    let width = f32::from(scroll.bounds().size.width) - 32.;
    ((width + 12.) / (104. + 12.)).floor().max(1.) as usize
}

/// Entry index range worth fetching thumbnails for. Before the first layout
/// the handle has no bounds, so fall back to a first-screenful guess.
fn visible_range(
    scroll: &ScrollHandle,
    row_h: f32,
    per_row: usize,
    total: usize,
) -> std::ops::Range<usize> {
    let viewport = f32::from(scroll.bounds().size.height);
    let (first_row, rows) = if viewport <= 0. {
        (0, 12)
    } else {
        let top = (-f32::from(scroll.offset().y)).max(0.);
        (
            (top / row_h) as usize,
            (viewport / row_h).ceil() as usize + 1,
        )
    };
    let start = first_row.saturating_sub(OVERSCAN_ROWS) * per_row;
    let end = ((first_row + rows + OVERSCAN_ROWS) * per_row).min(total);
    start.min(end)..end
}

/// Hands the thumbable entries in `range` to the workspace fetch queue.
fn want_visible(
    ws: &mut Workspace,
    entries: &[Entry],
    range: std::ops::Range<usize>,
    cx: &mut Context<Workspace>,
) {
    let paths = entries[range]
        .iter()
        .filter(|e| thumbable(e))
        .map(|e| e.path.clone())
        .collect();
    ws.want_thumbnails(paths, cx);
}

fn table_view(
    entries: &[Entry],
    thumbs: &HashMap<String, Arc<gpui_kit::RenderImage>>,
    scroll: &ScrollHandle,
    selected_set: &HashSet<String>,
    anchor: Option<&str>,
    hovered: Option<&str>,
    cx: &mut Context<Workspace>,
) -> ContextMenu<Stateful<Div>> {
    // One context menu for the whole listing (§7.3): rows stamp the pressed
    // entry into `target`; the container's menu builds in a deferred frame,
    // after the bubble pass, so it can pick entry vs. folder menu.
    let target: Rc<Cell<Option<String>>> = Rc::new(Cell::new(None));
    let menu = browser_menu(target.clone(), entries);
    v_flex()
        .id("file-table")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(scroll)
        .child(
            h_flex()
                .h(px(32.))
                .px_4()
                .gap_2()
                .items_center()
                .flex_shrink_0()
                .border_b_1()
                .border_color(ui::theme::SLATE_200)
                .bg(ui::theme::SLATE_50)
                .child(div().flex_1().child(label("Name")))
                .child(div().w(px(96.)).child(label("Modified")))
                .child(div().w(px(72.)).flex().justify_end().child(label("Size")))
                .child(div().w(px(72.)).child(label("Type")))
                .child(div().w(px(80.)).flex_shrink_0()),
        )
        .children(
            entries
                .iter()
                .map(|entry| {
                    table_row(
                        entry,
                        thumbs.get(&entry.path),
                        is_picked(selected_set, anchor, &entry.path),
                        hovered == Some(entry.path.as_str()),
                        target.clone(),
                        cx,
                    )
                })
                .collect::<Vec<_>>(),
        )
        .child(div().id("table-filler").h(px(96.)).flex_shrink_0())
        // Background right-clicks clear the row target so the deferred
        // menu build falls back to the folder menu. Rows stop propagation,
        // keeping their stamped target intact.
        .on_mouse_down(MouseButton::Right, {
            let target = target.clone();
            move |_, _, _| target.set(None)
        })
        .context_menu(menu)
}

/// The listing's single right-click menu: entry menu for a stamped row
/// target, folder menu for the background (§7.3).
fn browser_menu(target: Rc<Cell<Option<String>>>, entries: &[Entry]) -> MenuBuilder {
    let entries = entries.to_vec();
    Box::new(move |menu, window, cx| {
        match target
            .take()
            .as_deref()
            .and_then(|path| entries.iter().find(|e| e.path == path))
        {
            Some(entry) => entry_menu(entry)(menu, window, cx),
            None => folder_menu()(menu, window, cx),
        }
    })
}

/// Single-click selects (and opens the §9 inspector); double-click activates.
/// Right-click selects silently and stamps the menu target; the row carries
/// no menu of its own — the listing's single container menu serves it.
fn table_row(
    entry: &Entry,
    thumb: Option<&Arc<gpui_kit::RenderImage>>,
    selected: bool,
    hovered: bool,
    target: Rc<Cell<Option<String>>>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let (glyph, tint) = icon::entry_glyph(entry);
    let dir = entry.dir;
    let modified = format_relative_time(NOW_MS, entry.mtime);
    let size = if dir {
        entry
            .item_count
            .map(|n| format!("{n} items"))
            .unwrap_or_else(|| "—".into())
    } else {
        format_bytes(entry.size, 1)
    };
    let kind = FileCategory::of(entry).to_string();
    let path = entry.path.clone();
    let right_path = entry.path.clone();
    // Hover tracking for the row actions: enters win, an exit only clears
    // when this row still owns the hover (adjacent-row transitions fire
    // enter/exit in either order).
    let hover_handle = cx.entity();
    let hover_path = entry.path.clone();
    h_flex()
        .id(gpui_kit::SharedString::from(format!("row-{}", entry.path)))
        .a11y_item(format!("{}, {}, {}", entry.name, kind, size), selected)
        // Dropping files onto a folder uploads into that folder.
        .when(dir, |el| {
            let drop_path = entry.path.clone();
            el.drag_over::<ExternalPaths>(|s, _, _, _| {
                s.bg(ui::theme::DROP_BG)
                    .border_2()
                    .border_color(ui::theme::DROP_RING)
            })
            .on_drop::<ExternalPaths>(cx.listener(
                move |this, paths: &ExternalPaths, _w, cx| {
                    cx.stop_propagation();
                    this.drop_upload(Some(drop_path.clone()), paths, cx);
                },
            ))
        })
        .h(px(44.))
        .flex_shrink_0()
        .px_4()
        .gap_2()
        .items_center()
        .border_b_1()
        .border_color(ui::theme::SLATE_100)
        .cursor_pointer()
        .when(selected, |el| el.bg(ui::theme::SLATE_100))
        .when(!selected, |el| el.hover(|s| s.bg(ui::theme::SLATE_50)))
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_2()
                .items_center()
                .child(match thumb {
                    Some(rendered) => gpui_kit::img(rendered.clone())
                        .size(px(28.))
                        .flex_shrink_0()
                        .rounded_md()
                        .into_any_element(),
                    None => icon::icon(glyph, px(16.), tint).into_any_element(),
                })
                .when_some(entry.color_tag, |el, tag| {
                    el.child(
                        div()
                            .size(px(8.))
                            .rounded_full()
                            .bg(ui::theme::tag_color(tag)),
                    )
                })
                .when(entry.is_pinned, |el| {
                    el.child(icon::icon(STAR_FILL, px(11.), ui::theme::AMBER_500))
                })
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_size(px(13.))
                        .text_color(if dir {
                            ui::theme::BLACK
                        } else {
                            ui::theme::SLATE_700
                        })
                        .child(entry.name.clone()),
                ),
        )
        .child(
            div()
                .w(px(96.))
                .flex_shrink_0()
                .text_size(px(12.))
                .text_color(ui::theme::SLATE_500)
                .child(modified),
        )
        .child(
            div()
                .w(px(72.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .text_size(px(12.))
                .font_family(ui::theme::FONT_MONO)
                .text_color(ui::theme::SLATE_500)
                .child(size),
        )
        .child(
            div()
                .w(px(72.))
                .flex_shrink_0()
                .truncate()
                .text_size(px(12.))
                .text_color(ui::theme::SLATE_400)
                .child(kind),
        )
        .child(row_actions(entry, hovered, cx))
        .on_click(cx.listener(move |this, event: &ClickEvent, _w, cx| {
            let modifiers = event.modifiers();
            this.row_clicked(
                path.clone(),
                dir,
                modifiers.platform,
                modifiers.shift,
                event.click_count() >= 2,
                cx,
            );
        }))
        .on_hover(move |is_hovered: &bool, _w, cx: &mut App| {
            hover_handle.update(cx, |ws, cx| {
                if *is_hovered {
                    ws.state.hovered_row = Some(hover_path.clone());
                } else if ws.state.hovered_row.as_deref() == Some(hover_path.as_str()) {
                    ws.state.hovered_row = None;
                } else {
                    return;
                }
                cx.notify();
            });
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, _e, _w, cx| {
                this.right_click_entry(right_path.clone(), cx);
                target.set(Some(right_path.clone()));
                // Keep the stamped target: the container's background
                // clear-handler sits later in the bubble order.
                cx.stop_propagation();
            }),
        )
}

/// Trailing row cell: fixed-width so the table never reflows; the Cut,
/// Download and Delete actions render only while the row is hovered (§7.3).
fn row_actions(entry: &Entry, hovered: bool, cx: &mut Context<Workspace>) -> AnyElement {
    let cell = h_flex()
        .w(px(80.))
        .flex_shrink_0()
        .gap_1()
        .items_center()
        .justify_end();
    if !hovered {
        return cell.into_any_element();
    }
    // Swallow the press so the row's own click handler never sees a click
    // that started on an action button (it would select + open the inspector).
    let cell = cell.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
    let cut_path = entry.path.clone();
    let mut cell = cell.child(
        Button::new(gpui_kit::SharedString::from(format!("cut-{}", entry.path)))
            .ghost()
            .icon(Icon::empty().path(ARROW_UP_LEFT))
            .with_size(px(19.))
            .size(px(24.))
            .rounded(px(6.))
            .text_color(ui::theme::SLATE_400)
            .tooltip("Cut (paste into a folder to move)")
            .accessibility_label("Cut")
            .on_click(cx.listener(move |this, _e, _w, cx| {
                cx.stop_propagation();
                this.select_only(cut_path.clone(), cx);
                this.cut_selected(cx);
            })),
    );
    if !entry.dir {
        let dl_path = entry.path.clone();
        cell = cell.child(
            Button::new(gpui_kit::SharedString::from(format!("dl-{}", entry.path)))
                .ghost()
                .icon(Icon::empty().path(DOWNLOAD_SIMPLE))
                .with_size(px(19.))
                .size(px(24.))
                .rounded(px(6.))
                .text_color(ui::theme::SLATE_400)
                .tooltip("Download")
                .accessibility_label("Download")
                .on_click(cx.listener(move |this, _e, _w, cx| {
                    cx.stop_propagation();
                    this.select_only(dl_path.clone(), cx);
                    this.download_selected(cx);
                })),
        );
    }
    let rm_path = entry.path.clone();
    cell.child(
        Button::new(gpui_kit::SharedString::from(format!("rm-{}", entry.path)))
            .ghost()
            .icon(Icon::empty().path(TRASH))
            .with_size(px(19.))
            .size(px(24.))
            .rounded(px(6.))
            .text_color(ui::theme::ROSE_500)
            .tooltip("Delete")
            .accessibility_label("Delete")
            .on_click(cx.listener(move |this, _e, _w, cx| {
                cx.stop_propagation();
                this.select_only(rm_path.clone(), cx);
                this.delete_selected(cx);
            })),
    )
    .into_any_element()
}

/// Right-click menu for one entry (§7.3). The right-click has already
/// selected the entry, so every item can act on the selection. Download is
/// file-only (folder downloads arrive with R7). Pin and color tags are
/// desktop-local decorations.
fn entry_menu(entry: &Entry) -> MenuBuilder {
    let dir = entry.dir;
    let pinned = entry.is_pinned;
    let tagged = entry.color_tag;
    Box::new(move |menu, _window, _cx| {
        let menu = menu
            .menu(if dir { "Open" } else { "Preview" }, Box::new(OpenSelected))
            .menu("Rename…", Box::new(RenameSelected))
            .menu("Copy", Box::new(CopySelected))
            .menu("Cut", Box::new(CutSelected))
            .menu("Copy Path", Box::new(CopySelectedPath));
        let menu = if dir {
            menu
        } else {
            menu.menu("Download", Box::new(DownloadSelected))
        };
        let menu = menu.separator().menu(
            if pinned {
                "Unpin from Quick Access"
            } else {
                "Pin to Quick Access"
            },
            Box::new(TogglePin),
        );
        let menu = menu.separator();
        let menu = tag_item(menu, ColorTag::Blue, tagged, Box::new(TagBlue));
        let menu = tag_item(menu, ColorTag::Emerald, tagged, Box::new(TagEmerald));
        let menu = tag_item(menu, ColorTag::Amber, tagged, Box::new(TagAmber));
        let menu = tag_item(menu, ColorTag::Purple, tagged, Box::new(TagPurple));
        let menu = tag_item(menu, ColorTag::Rose, tagged, Box::new(TagRose));
        let menu = tag_item(menu, ColorTag::Slate, tagged, Box::new(TagSlate));
        menu.menu("Clear Tag", Box::new(ClearTag))
            .separator()
            .menu("Delete", Box::new(DeleteSelected))
    })
}

/// One color-tag menu row: swatch + name, checked when already applied.
fn tag_item(
    menu: PopupMenu,
    tag: ColorTag,
    current: Option<ColorTag>,
    action: Box<dyn gpui_kit::Action>,
) -> PopupMenu {
    menu.menu_element_with_check(current == Some(tag), action, move |_window, _cx| {
        h_flex()
            .gap_1p5()
            .items_center()
            .child(
                div()
                    .size(px(9.))
                    .rounded_full()
                    .bg(ui::theme::tag_color(tag)),
            )
            .child(div().text_size(px(12.)).child(tag.as_str().to_string()))
    })
}

/// Right-click menu for the folder background (§7.1 empty space). Paste
/// lands any staged cut/copy here; it no-ops (with a toast) when nothing
/// is staged.
fn folder_menu() -> MenuBuilder {
    Box::new(|menu, _window, _cx| {
        menu.menu("New Folder…", Box::new(NewFolder))
            .menu("Upload…", Box::new(UploadFiles))
            .separator()
            .menu("Paste Into This Folder", Box::new(PasteIntoFolder))
            .separator()
            .menu("Copy Current Path", Box::new(CopyCurrentPath))
    })
}

/// §7.3 Finder-style grid: borderless centered tiles; the slate tint tracks
/// hover, press and selection. The scroll container carries the folder menu
/// for the empty area; the filler under the tiles is a fixed breathing gap.
fn grid_view(
    entries: &[Entry],
    thumbs: &HashMap<String, Arc<gpui_kit::RenderImage>>,
    scroll: &ScrollHandle,
    selected_set: &HashSet<String>,
    anchor: Option<&str>,
    hovered: Option<&str>,
    cx: &mut Context<Workspace>,
) -> ContextMenu<Stateful<Div>> {
    // Same single-menu scheme as the table: tiles stamp the pressed entry;
    // the container's deferred build picks entry vs. folder menu.
    let target: Rc<Cell<Option<String>>> = Rc::new(Cell::new(None));
    let menu = browser_menu(target.clone(), entries);
    v_flex()
        .id("file-grid-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(scroll)
        .p_4()
        .child(
            div()
                .id("file-grid")
                .flex()
                .flex_wrap()
                .gap_3()
                .children(entries.iter().map(|entry| {
                    let thumb = thumbs.get(&entry.path);
                    grid_card(
                        entry,
                        thumb,
                        is_picked(selected_set, anchor, &entry.path),
                        hovered == Some(entry.path.as_str()),
                        target.clone(),
                        cx,
                    )
                })),
        )
        .child(div().id("grid-filler").h(px(96.)).flex_shrink_0())
        .on_mouse_down(MouseButton::Right, {
            let target = target.clone();
            move |_, _, _| target.set(None)
        })
        .context_menu(menu)
}

/// One Finder tile: 40px glyph (or a fetched thumbnail for images),
/// centered name and size caption, no border — selection is a rounded slate
/// wash, like Finder's icon selection.
fn grid_card(
    entry: &Entry,
    thumb: Option<&Arc<gpui_kit::RenderImage>>,
    selected: bool,
    hovered: bool,
    target: Rc<Cell<Option<String>>>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let (glyph, tint) = icon::entry_glyph(entry);
    let dir = entry.dir;
    let size = if dir {
        entry
            .item_count
            .map(|n| format!("{n} items"))
            .unwrap_or_else(|| "Folder".into())
    } else {
        format_bytes(entry.size, 1)
    };
    let path = entry.path.clone();
    let right_path = entry.path.clone();
    let rm_path = entry.path.clone();
    // Hover tracking for the trash overlay, identical to table rows.
    let hover_handle = cx.entity();
    let hover_path = entry.path.clone();
    v_flex()
        .id(gpui_kit::SharedString::from(format!("tile-{}", entry.path)))
        .a11y_item(entry.name.clone(), selected)
        // Dropping files onto a folder uploads into that folder.
        .when(dir, |el| {
            let drop_path = entry.path.clone();
            el.drag_over::<ExternalPaths>(|s, _, _, _| {
                s.bg(ui::theme::DROP_BG)
                    .border_2()
                    .border_color(ui::theme::DROP_RING)
            })
            .on_drop::<ExternalPaths>(cx.listener(
                move |this, paths: &ExternalPaths, _w, cx| {
                    cx.stop_propagation();
                    this.drop_upload(Some(drop_path.clone()), paths, cx);
                },
            ))
        })
        .relative()
        .w(px(104.))
        .p_2()
        .gap_1()
        .items_center()
        .flex_shrink_0()
        .rounded_lg()
        .cursor_pointer()
        .when(selected, |el| el.bg(ui::theme::SLATE_100))
        .when(!selected, |el| {
            el.hover(|s| s.bg(ui::theme::SLATE_50))
                .active(|s| s.bg(ui::theme::SLATE_200))
        })
        .child(match thumb {
            Some(rendered) => gpui_kit::img(rendered.clone())
                .size(px(84.))
                .rounded_lg()
                .into_any_element(),
            None => icon::icon(glyph, px(40.), tint).into_any_element(),
        })
        .when(entry.is_pinned || entry.color_tag.is_some(), |el| {
            el.child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .when_some(entry.color_tag, |el, tag| {
                        el.child(
                            div()
                                .size(px(7.))
                                .rounded_full()
                                .bg(ui::theme::tag_color(tag)),
                        )
                    })
                    .when(entry.is_pinned, |el| {
                        el.child(icon::icon(STAR_FILL, px(10.), ui::theme::AMBER_500))
                    }),
            )
        })
        .child(
            div()
                .w_full()
                .text_center()
                .truncate()
                .text_size(px(12.))
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if selected {
                    ui::theme::BLACK
                } else {
                    ui::theme::SLATE_700
                })
                .child(entry.name.clone()),
        )
        .child(
            div()
                .text_size(px(10.))
                .text_color(ui::theme::SLATE_400)
                .child(size),
        )
        // Hover trash (§7.3 parity with rows): a small floating button in
        // the tile's top-right corner; both press and click stop
        // propagating so the tile never selects/opens underneath.
        .when(hovered, |el| {
            el.child(
                h_flex()
                    .id(gpui_kit::SharedString::from(format!(
                        "tile-rm-{}",
                        entry.path
                    )))
                    .a11y_button(format!("Delete {}", entry.name))
                    .absolute()
                    .top_1()
                    .right_1()
                    .size(px(22.))
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(ui::theme::WHITE)
                    .border_1()
                    .border_color(ui::theme::ROSE_500.opacity(0.4))
                    .shadow_sm()
                    .cursor_pointer()
                    .child(icon::icon(TRASH, px(12.), ui::theme::ROSE_500))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _e, _w, cx| {
                        this.select_only(rm_path.clone(), cx);
                        this.delete_selected(cx);
                    })),
            )
        })
        .on_click(cx.listener(move |this, event: &ClickEvent, _w, cx| {
            let modifiers = event.modifiers();
            this.row_clicked(
                path.clone(),
                dir,
                modifiers.platform,
                modifiers.shift,
                event.click_count() >= 2,
                cx,
            );
        }))
        .on_hover(move |is_hovered: &bool, _w, cx: &mut App| {
            hover_handle.update(cx, |ws, cx| {
                if *is_hovered {
                    ws.state.hovered_row = Some(hover_path.clone());
                } else if ws.state.hovered_row.as_deref() == Some(hover_path.as_str()) {
                    ws.state.hovered_row = None;
                } else {
                    return;
                }
                cx.notify();
            });
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, _e, _w, cx| {
                this.right_click_entry(right_path.clone(), cx);
                target.set(Some(right_path.clone()));
                cx.stop_propagation();
            }),
        )
}

/// §8.2 empty state for an active quick-access filter (virtual folder):
/// one message per category; no folder menu — there is no real folder
/// behind the view.
fn filter_empty_state(filter: &QuickFilter) -> Stateful<Div> {
    let (glyph, tint, title, hint) = match filter {
        QuickFilter::Downloads => (
            DOWNLOAD_SIMPLE,
            ui::theme::EMERALD_600,
            "No downloads",
            "Files you download on the phone land in ~/Download",
        ),
        QuickFilter::Images => (
            IMAGE,
            ui::theme::AMBER_500,
            "No images",
            "Photos and pictures on the phone appear here",
        ),
        QuickFilter::Videos => (
            FILM_STRIP,
            ui::theme::PURPLE_500,
            "No videos",
            "Videos on the phone appear here",
        ),
        QuickFilter::Audio => (
            MUSIC_NOTES,
            ui::theme::SKY_500,
            "No audio",
            "Music and recordings on the phone appear here",
        ),
        QuickFilter::Documents => (
            FILE_TEXT,
            ui::theme::EMERALD_500,
            "No documents",
            "Documents, notes and code files appear here",
        ),
        QuickFilter::Tag(tag) => (
            FUNNEL_SIMPLE,
            ui::theme::tag_color(*tag),
            "Nothing tagged",
            "Tag files from their right-click menu to collect them here",
        ),
    };
    div()
        .id("browser-filter-empty")
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .child(
            v_flex()
                .items_center()
                .gap_2()
                .child(icon::icon(glyph, px(40.), tint))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ui::theme::SLATE_700)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(ui::theme::SLATE_400)
                        .child(hint),
                ),
        )
}

/// §7.3 empty-directory placeholder; right-click carries the folder
/// menu. Offline (no paired phone) the message points at pairing.
fn empty_state(status: LinkStatus) -> ContextMenu<Stateful<Div>> {
    let (glyph, title, hint) = match status {
        LinkStatus::Offline => (
            DEVICE_MOBILE,
            "No device connected",
            "Pair your phone from the device chip (top right) — files appear here once connected",
        ),
        _ => (
            FOLDER_OPEN,
            "This folder is empty",
            "Upload files or create a folder to get started",
        ),
    };
    div()
        .id("browser-empty")
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .child(
            v_flex()
                .items_center()
                .gap_2()
                .child(icon::icon(glyph, px(40.), ui::theme::SLATE_300))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ui::theme::SLATE_700)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(ui::theme::SLATE_400)
                        .child(hint),
                ),
        )
        .context_menu(folder_menu())
}

/// Toolbar label for the current sort order (§7.1).
fn sort_label(sort: &SortSpec) -> String {
    let field = match sort.field {
        SortField::Name => "Name",
        SortField::Size => "Size",
        SortField::Mtime => "Modified",
        SortField::Type => "Type",
    };
    let arrow = match sort.direction {
        SortDirection::Asc => "↑",
        SortDirection::Desc => "↓",
    };
    format!("{field} {arrow}")
}
