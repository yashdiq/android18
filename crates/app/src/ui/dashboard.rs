//! §10 dashboard mirroring the React prototype's `DashboardView`: a banner
//! with Refresh / Browse Storage actions, the primary-storage volume card
//! (usage headline, multi-segment bar, dot legend), the storage-volumes
//! analytics card grid, and the recent-activity table with All / Files /
//! Folders tabs plus instant search. Byte figures follow the prototype's
//! display model (see [`crate::state::AppState::dashboard_categories`]);
//! counts are real.

use crate::ui::A11y as _;
use android18_core::domain::{Device, Entry};
use android18_core::fs::paths::STORAGE_ROOT;
use android18_core::util::format::{format_bytes, format_relative_time};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement, SharedString, Stateful, StatefulInteractiveElement as _, Styled, div, px,
    relative,
};

use crate::icon::{
    self, ARROW_CLOCKWISE, CARET_RIGHT, CHART_PIE, CLOCK, DOWNLOAD_SIMPLE, EYE, FOLDER_OPEN,
    MAGNIFYING_GLASS, STACK,
};
use crate::state::{CategorySlice, RecentFilter};
use crate::ui::{self, NOW_MS, card, label};
use crate::workspace::Workspace;

/// The prototype's recents table renders at most 35 rows.
const RECENT_ROWS: usize = 35;

/// Page body: banner over a centered ~1280px column holding the storage
/// card, analytics grid and recents table.
pub fn render(ws: &mut Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    let info = ws.state.device_info.clone();
    let used = ws.state.storage_used;
    let total = info.storage_total_bytes;
    let free = total.saturating_sub(used);
    let used_percent = if total > 0 {
        (used as f64 / total as f64 * 100.).round() as u64
    } else {
        0
    };
    let file_count = ws.state.all_entries.iter().filter(|e| !e.dir).count();
    let folder_count = ws.state.all_entries.iter().filter(|e| e.dir).count();
    let categories = ws.state.dashboard_categories();
    let filter = ws.state.recent_filter;
    let query = ws.recent_search_input.read(cx).value().to_string();
    let search_input = ws.recent_search_input.clone();
    let recents = ws
        .state
        .recents_filtered(filter, &query, RECENT_ROWS)
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();

    v_flex()
        .id("dashboard")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .bg(ui::theme::SLATE_50)
        .child(banner(&info, cx))
        .child(
            h_flex().w_full().flex_shrink_0().justify_center().child(
                v_flex()
                    .w_full()
                    .max_w(px(1280.))
                    .p_5()
                    .gap_4()
                    .min_w_0()
                    .child(storage_card(used, total, free, used_percent, &categories))
                    .child(analytics_section(folder_count, file_count, &categories, cx))
                    .child(recents_card(filter, &search_input, &recents, cx)),
            ),
        )
        .into_any_element()
}

/// Page header: title, model chip and mono root line on the left; the
/// prototype's Refresh (outline) and Browse Storage (dark) buttons right.
fn banner(info: &Device, cx: &mut Context<Workspace>) -> Div {
    h_flex()
        .flex_shrink_0()
        .px_6()
        .py_4()
        .gap_3()
        .items_center()
        .border_b_1()
        .border_color(ui::theme::SLATE_200)
        .bg(ui::theme::WHITE)
        .child(
            v_flex()
                .min_w_0()
                .gap_1()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(16.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(ui::theme::BLACK)
                                .child("Storage & Volumes Dashboard"),
                        )
                        .child(
                            div()
                                .h(px(18.))
                                .px_2()
                                .flex()
                                .items_center()
                                .rounded_md()
                                .border_1()
                                .border_color(ui::theme::SLATE_200)
                                .bg(ui::theme::SLATE_100)
                                .font_family(ui::theme::FONT_MONO)
                                .text_size(px(11.))
                                .text_color(ui::theme::SLATE_600)
                                .child(info.model.clone()),
                        ),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .font_family(ui::theme::FONT_MONO)
                        .text_color(ui::theme::SLATE_500)
                        .child(format!("{STORAGE_ROOT} · Android {}", info.android_version)),
                ),
        )
        .child(div().flex_1())
        .child(
            Button::new("dashboard-refresh")
                .custom(
                    ButtonCustomVariant::new(cx)
                        .color(ui::theme::WHITE)
                        .hover(ui::theme::SLATE_50)
                        .active(ui::theme::SLATE_100),
                )
                .h(px(28.))
                .px(px(12.))
                .rounded(ui::theme::RADIUS_LG)
                .border_1()
                .border_color(ui::theme::SLATE_200)
                .shadow_2xs()
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(icon::icon(ARROW_CLOCKWISE, px(14.), ui::theme::SLATE_500))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(ui::theme::SLATE_700)
                                .child("Refresh"),
                        ),
                )
                .tooltip("Refresh storage analysis")
                .accessibility_label("Refresh storage analysis")
                .on_click(cx.listener(|this, _e, _w, cx| this.refresh_dashboard(cx))),
        )
        .child(
            Button::new("dashboard-browse")
                .custom(
                    ButtonCustomVariant::new(cx)
                        .color(ui::theme::SLATE_900)
                        .hover(ui::theme::SLATE_800)
                        .active(ui::theme::SLATE_700),
                )
                .h(px(28.))
                .px(px(14.))
                .rounded(ui::theme::RADIUS_LG)
                .shadow_2xs()
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(icon::icon(FOLDER_OPEN, px(14.), ui::theme::WHITE))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(ui::theme::WHITE)
                                .child("Browse Storage"),
                        ),
                )
                .tooltip("Open the file browser")
                .accessibility_label("Open the file browser")
                .on_click(cx.listener(|this, _e, _w, cx| this.browse_storage(cx))),
        )
}

/// §2.5 primary-volume headline: chart-pie tile, volume name, mono
/// used/total/available figures, then the multi-segment bar and dot legend.
fn storage_card(
    used: u64,
    total: u64,
    free: u64,
    used_percent: u64,
    categories: &[CategorySlice],
) -> Div {
    card().p_4().child(
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .gap(px(10.))
                    .items_center()
                    .child(
                        div()
                            .size(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .bg(ui::theme::SLATE_100)
                            .child(icon::icon(CHART_PIE, px(18.), ui::theme::SLATE_700)),
                    )
                    .child(
                        v_flex()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(ui::theme::BLACK)
                                    .child("Primary Storage Volume"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(ui::theme::SLATE_500)
                                    .child("Internal shared flash storage distribution"),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .gap_2()
                            .items_baseline()
                            .font_family(ui::theme::FONT_MONO)
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(ui::theme::BLACK)
                                    .child(format_bytes(used, 1)),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(ui::theme::SLATE_400)
                                    .child(format!(
                                        "/ {} ({used_percent}%)",
                                        format_bytes(total, 1)
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(ui::theme::EMERALD_600)
                                    .child(format!("{} available", format_bytes(free, 1))),
                            ),
                    ),
            )
            .child(segmented_bar(categories, total))
            .child(legend(categories)),
    )
}

/// 12px usage bar: one segment per category, width proportional to the
/// bucket's share of the advertised total (the container clips the ends).
fn segmented_bar(categories: &[CategorySlice], total: u64) -> Div {
    h_flex()
        .h(px(12.))
        .w_full()
        .rounded_full()
        .overflow_hidden()
        .bg(ui::theme::SLATE_100)
        .children(categories.iter().map(|slice| {
            let share = if total > 0 {
                slice.bytes as f32 / total as f32
            } else {
                0.
            };
            div().h_full().bg(slice.color).w(relative(share))
        }))
}

/// Dot legend below the bar: color swatch, label, mono display bytes.
fn legend(categories: &[CategorySlice]) -> Div {
    h_flex()
        .flex_wrap()
        .gap(px(14.))
        .children(categories.iter().map(|slice| {
            h_flex()
                .gap(px(6.))
                .items_center()
                .child(div().size(px(10.)).rounded_sm().bg(slice.color))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(ui::theme::SLATE_700)
                        .child(slice.label),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .font_family(ui::theme::FONT_MONO)
                        .text_color(ui::theme::SLATE_400)
                        .child(format_bytes(slice.bytes, 1)),
                )
        }))
}

/// §10 volume-analytics grid: section header (folder/file tallies) over the
/// six category cards, three per row.
fn analytics_section(
    folder_count: usize,
    file_count: usize,
    categories: &[CategorySlice],
    cx: &mut Context<Workspace>,
) -> Div {
    v_flex()
        .gap_3()
        .w_full()
        .child(
            h_flex()
                .items_center()
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(icon::icon(STACK, px(12.), ui::theme::SLATE_400))
                        .child(label("Storage Volumes Analytics")),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(ui::theme::SLATE_400)
                        .child(format!("{folder_count} folders · {file_count} files")),
                ),
        )
        .child(
            h_flex().flex_wrap().gap_3().w_full().children(
                categories
                    .iter()
                    .map(|slice| category_card(slice, cx))
                    .collect::<Vec<_>>(),
            ),
        )
}

/// One volume-category card: tinted glyph tile, label, real item count,
/// chevron, and a footer with the display bytes plus a Browse affordance.
/// The whole card navigates to the bucket's folder.
fn category_card(slice: &CategorySlice, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let path = slice
        .folder_path
        .clone()
        .unwrap_or_else(|| STORAGE_ROOT.to_string());
    let items = format!(
        "{} item{}",
        slice.count,
        if slice.count == 1 { "" } else { "s" }
    );
    v_flex()
        .id(SharedString::from(format!("volume-{}", slice.label)))
        .a11y_button(format!("{} volume", slice.label))
        .flex_1()
        .min_w(px(256.))
        .gap_3()
        .p_4()
        .bg(ui::theme::WHITE)
        .border_1()
        .border_color(ui::theme::SLATE_200)
        .rounded_xl()
        .cursor_pointer()
        .hover(|s| s.border_color(ui::theme::SLATE_300))
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .child(
                    div()
                        .size(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_lg()
                        .bg(slice.color.opacity(0.10))
                        .child(icon::icon(slice.glyph, px(18.), slice.color)),
                )
                .child(
                    v_flex()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(ui::theme::BLACK)
                                .child(slice.label),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_family(ui::theme::FONT_MONO)
                                .text_color(ui::theme::SLATE_500)
                                .child(items),
                        ),
                )
                .child(div().flex_1())
                .child(icon::icon(CARET_RIGHT, px(14.), ui::theme::SLATE_300)),
        )
        .child(
            h_flex()
                .pt_3()
                .items_center()
                .border_t_1()
                .border_color(ui::theme::SLATE_100)
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .font_family(ui::theme::FONT_MONO)
                        .text_color(ui::theme::BLACK)
                        .child(format_bytes(slice.bytes, 1)),
                )
                .child(div().flex_1())
                .child(
                    h_flex()
                        .gap(px(2.))
                        .items_center()
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(ui::theme::SLATE_400)
                                .child("Browse"),
                        )
                        .child(icon::icon(CARET_RIGHT, px(10.), ui::theme::SLATE_400)),
                ),
        )
        .on_click(cx.listener(move |this, _e, _w, cx| {
            this.navigate(path.clone(), cx);
        }))
}

/// §10 recent-activity card: header with clock icon, All / Files / Folders
/// tabs and the instant-search box, then a 35-row table over the whole tree
/// (newest first). Rows navigate into folders and inspect files.
fn recents_card(
    filter: RecentFilter,
    search: &Entity<InputState>,
    recents: &[Entry],
    cx: &mut Context<Workspace>,
) -> Div {
    card().p_0().overflow_hidden().child(
        v_flex()
            .child(
                h_flex()
                    .px_4()
                    .py_3()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(ui::theme::SLATE_200)
                    .bg(ui::theme::SLATE_50)
                    .child(icon::icon(CLOCK, px(14.), ui::theme::SLATE_500))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(ui::theme::BLACK)
                            .child("Recent Files & Folders Activity"),
                    )
                    .child(div().flex_1())
                    .child(filter_tabs(filter, cx))
                    .child(search_field(search)),
            )
            .child(table_header())
            .children(
                recents
                    .iter()
                    .map(|entry| recent_row(entry, cx))
                    .collect::<Vec<_>>(),
            )
            .when(recents.is_empty(), |el| {
                el.child(
                    div()
                        .py_8()
                        .flex()
                        .justify_center()
                        .text_size(px(12.))
                        .text_color(ui::theme::SLATE_400)
                        .child("No recent files or folders match your query."),
                )
            }),
    )
}

/// Segmented All / Files / Folders control on a slate track; the active
/// segment sits on a white pill.
fn filter_tabs(active: RecentFilter, cx: &mut Context<Workspace>) -> Div {
    h_flex()
        .h(px(26.))
        .p(px(2.))
        .gap_0p5()
        .rounded_lg()
        .border_1()
        .border_color(ui::theme::SLATE_200)
        .bg(ui::theme::SLATE_100)
        .children(
            RecentFilter::ALL
                .into_iter()
                .map(|filter| filter_tab(filter, filter == active, cx))
                .collect::<Vec<_>>(),
        )
}

fn filter_tab(filter: RecentFilter, active: bool, cx: &mut Context<Workspace>) -> Stateful<Div> {
    h_flex()
        .id(SharedString::from(format!("recent-tab-{}", filter.label())))
        .a11y_item(filter.label(), active)
        .h_full()
        .px_2()
        .items_center()
        .rounded_md()
        .cursor_pointer()
        .text_size(px(11.))
        .when(active, |el| {
            el.bg(ui::theme::WHITE)
                .text_color(ui::theme::BLACK)
                .font_weight(FontWeight::SEMIBOLD)
        })
        .when(!active, |el| {
            el.text_color(ui::theme::SLATE_600)
                .hover(|s| s.text_color(ui::theme::BLACK))
        })
        .child(filter.label())
        .on_click(cx.listener(move |this, _e, _w, cx| {
            this.state.recent_filter = filter;
            cx.notify();
        }))
}

/// Instant-search box with the prototype's inline magnifier prefix.
fn search_field(search: &Entity<InputState>) -> Div {
    div().w(px(200.)).flex_shrink_0().child(
        Input::new(search).id("recent-search").prefix(icon::icon(
            MAGNIFYING_GLASS,
            px(12.),
            ui::theme::SLATE_400,
        )),
    )
}

/// Column captions for the recents table; widths match [`recent_row`].
fn table_header() -> Div {
    h_flex()
        .h(px(28.))
        .flex_shrink_0()
        .px_4()
        .gap_2()
        .items_center()
        .bg(ui::theme::SLATE_50)
        .border_b_1()
        .border_color(ui::theme::SLATE_200)
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(ui::theme::SLATE_500)
        .child(div().flex_1().min_w_0().child("Name"))
        .child(div().w(px(180.)).flex_shrink_0().child("Path Location"))
        .child(
            div()
                .w(px(76.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .child("Size"),
        )
        .child(
            div()
                .w(px(84.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .child("Modified"),
        )
        .child(
            div()
                .w(px(64.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .child("Actions"),
        )
}

/// Inspect plus open-folder (dirs) or download (files) ghost buttons.
fn recent_actions(entry: &Entry, cx: &mut Context<Workspace>) -> Div {
    let inspect_path = entry.path.clone();
    let action_path = entry.path.clone();
    let mut actions = h_flex()
        .w(px(64.))
        .flex_shrink_0()
        .gap_0p5()
        .flex()
        .justify_end();
    actions = actions.child(
        ui::icon_button(
            SharedString::from(format!("recent-inspect-{}", entry.path)),
            EYE,
            "Inspect details",
        )
        .on_click(cx.listener(move |this, _e, _w, cx| {
            this.select_entry(Some(inspect_path.clone()), cx);
        })),
    );
    if entry.dir {
        actions = actions.child(
            ui::icon_button(
                SharedString::from(format!("recent-open-{}", entry.path)),
                FOLDER_OPEN,
                "Open folder",
            )
            .on_click(cx.listener(move |this, _e, _w, cx| this.navigate(action_path.clone(), cx))),
        );
    } else {
        actions = actions.child(
            ui::icon_button(
                SharedString::from(format!("recent-download-{}", entry.path)),
                DOWNLOAD_SIMPLE,
                "Download",
            )
            .on_click(cx.listener(move |this, _e, _w, cx| {
                this.select_entry(Some(action_path.clone()), cx);
                this.download_selected(cx);
            })),
        );
    }
    actions
}

/// One recents row: glyph + name (+ Folder badge), `~`-rooted path, size or
/// item count, relative mtime, then [`recent_actions`]. Clicking navigates
/// into folders and inspects files, mirroring the prototype.
fn recent_row(entry: &Entry, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let (glyph, tint) = icon::entry_glyph(entry);
    let path = entry.path.clone();
    let is_dir = entry.dir;
    let display_path = entry.path.replace(STORAGE_ROOT, "~");
    let size_label = match entry.item_count {
        Some(n) if entry.dir => format!("{n} items"),
        _ if entry.dir => "—".to_string(),
        _ => format_bytes(entry.size, 1),
    };
    h_flex()
        .id(SharedString::from(format!("recent-{}", entry.path)))
        .a11y_button(entry.name.clone())
        .h(px(34.))
        .px_4()
        .gap_2()
        .items_center()
        .bg(ui::theme::WHITE)
        .border_b_1()
        .border_color(ui::theme::SLATE_100)
        .cursor_pointer()
        .hover(|s| s.bg(ui::theme::SLATE_50))
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_2()
                .items_center()
                .child(icon::icon(glyph, px(15.), tint))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(ui::theme::SLATE_700)
                        .truncate()
                        .child(entry.name.clone()),
                )
                .when(entry.dir, |el| {
                    el.child(
                        div()
                            .h(px(15.))
                            .px_1()
                            .flex()
                            .items_center()
                            .rounded_sm()
                            .border_1()
                            .border_color(ui::theme::SLATE_200)
                            .bg(ui::theme::SLATE_100)
                            .text_size(px(9.))
                            .text_color(ui::theme::SLATE_600)
                            .child("Folder"),
                    )
                }),
        )
        .child(
            div()
                .w(px(180.))
                .flex_shrink_0()
                .truncate()
                .text_size(px(10.))
                .font_family(ui::theme::FONT_MONO)
                .text_color(ui::theme::SLATE_500)
                .child(display_path),
        )
        .child(
            div()
                .w(px(76.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .text_size(px(11.))
                .font_family(ui::theme::FONT_MONO)
                .text_color(ui::theme::SLATE_600)
                .child(size_label),
        )
        .child(
            div()
                .w(px(84.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .text_size(px(10.))
                .font_family(ui::theme::FONT_MONO)
                .text_color(ui::theme::SLATE_500)
                .child(format_relative_time(NOW_MS, entry.mtime)),
        )
        .child(recent_actions(entry, cx))
        .on_click(cx.listener(move |this, _e, _w, cx| {
            if is_dir {
                this.navigate(path.clone(), cx);
            } else {
                this.select_entry(Some(path.clone()), cx);
            }
        }))
}
