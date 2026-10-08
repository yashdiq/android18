//! §8 sidebar: quick access, the recursive folder tree and the device footer.

use crate::ui::A11y as _;
use android18_core::domain::{ColorTag, Entry};
use android18_core::fs::paths::STORAGE_ROOT;
use android18_core::util::format::format_bytes;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, InteractiveElement as _, IntoElement, ParentElement, Stateful,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};

use crate::icon::{
    self, CARET_DOWN, CARET_RIGHT, DOWNLOAD_SIMPLE, FILE_TEXT, FILM_STRIP, FOLDER, IMAGE,
    MUSIC_NOTES,
};
use crate::state::{AppState, QuickFilter};
use crate::ui::{self, SIDEBAR_W, label};
use crate::workspace::Workspace;

pub fn render(ws: &mut Workspace, _window: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    let tags = ws.state.tag_summary();
    v_flex()
        .id("folder-tree")
        .w(px(SIDEBAR_W))
        .flex_shrink_0()
        .h_full()
        .min_h_0()
        .border_r_1()
        .border_color(ui::theme::SLATE_200)
        .bg(ui::theme::SLATE_50)
        .child(
            v_flex()
                .id("tree-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_2()
                .gap_4()
                // §8.2 quick access: file-type virtual folders (counts from
                // the tree snapshot); clicking toggles the browser filter.
                .child(
                    v_flex()
                        .gap_1()
                        .child(label("Quick access").px_1().pb_1())
                        .children(
                            QuickFilter::TYPES
                                .iter()
                                .map(|filter| {
                                    filter_row(
                                        &ws.state,
                                        filter,
                                        ws.state.quick_filter == Some(filter.clone()),
                                        cx,
                                    )
                                })
                                .collect::<Vec<_>>(),
                        ),
                )
                // §8.2 tags in use across the snapshot; hidden while empty.
                .when(!tags.is_empty(), |el| {
                    el.child(
                        v_flex()
                            .gap_1()
                            .child(label("Tags").px_1().pb_1())
                            .children(
                                tags.iter()
                                    .map(|(tag, count)| {
                                        tag_row(
                                            *tag,
                                            *count,
                                            ws.state.quick_filter == Some(QuickFilter::Tag(*tag)),
                                            cx,
                                        )
                                    })
                                    .collect::<Vec<_>>(),
                            ),
                    )
                })
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(label("Folders").px_1().pb_1())
                        .child(root_row(&ws.state, cx))
                        .child(tree_children(&ws.state, cx, STORAGE_ROOT, 0)),
                ),
        )
        .child(footer(&ws.state))
        .into_any_element()
}

/// §8.2 (glyph, tint) for one quick-access type row.
fn filter_glyph(filter: &QuickFilter) -> (&'static str, gpui_kit::Hsla) {
    match filter {
        QuickFilter::Downloads => (DOWNLOAD_SIMPLE, ui::theme::EMERALD_600),
        QuickFilter::Images => (IMAGE, ui::theme::AMBER_500),
        QuickFilter::Videos => (FILM_STRIP, ui::theme::PURPLE_500),
        QuickFilter::Audio => (MUSIC_NOTES, ui::theme::SKY_500),
        QuickFilter::Documents => (FILE_TEXT, ui::theme::SLATE_600),
        QuickFilter::Tag(tag) => (FOLDER, ui::theme::tag_color(*tag)),
    }
}

fn filter_row(
    state: &AppState,
    filter: &QuickFilter,
    active: bool,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let (glyph, tint) = filter_glyph(filter);
    let count = state.quick_filter_count(filter);
    let target = filter.clone();
    h_flex()
        .id(gpui_kit::SharedString::from(format!(
            "qa-{}",
            filter.label()
        )))
        .a11y_item(format!("{} files", filter.label()), active)
        .h(px(28.))
        .px_2()
        .gap_2()
        .items_center()
        .rounded_md()
        .cursor_pointer()
        .when(active, |el| el.bg(ui::theme::SLATE_200))
        .when(!active, |el| el.hover(|s| s.bg(ui::theme::SLATE_100)))
        .child(icon::icon(
            glyph,
            px(14.),
            if active { ui::theme::SLATE_700 } else { tint },
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.))
                .text_color(if active {
                    ui::theme::BLACK
                } else {
                    ui::theme::SLATE_600
                })
                .child(filter.label()),
        )
        .child(
            div()
                .text_size(px(10.))
                .text_color(ui::theme::SLATE_400)
                .child(count.to_string()),
        )
        .on_click(cx.listener(move |this, _e, _w, cx| this.open_quick_filter(target.clone(), cx)))
}

/// One §8.2 tag row: colored dot, capitalized name, count; opens the tag
/// filter.
fn tag_row(
    tag: ColorTag,
    count: usize,
    active: bool,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let name = match tag {
        ColorTag::Blue => "Blue",
        ColorTag::Emerald => "Emerald",
        ColorTag::Amber => "Amber",
        ColorTag::Purple => "Purple",
        ColorTag::Rose => "Rose",
        ColorTag::Slate => "Slate",
    };
    h_flex()
        .id(gpui_kit::SharedString::from(format!(
            "tag-{}",
            tag.as_str()
        )))
        .a11y_item(format!("{name} tag"), active)
        .h(px(28.))
        .px_2()
        .gap_2()
        .items_center()
        .rounded_md()
        .cursor_pointer()
        .when(active, |el| el.bg(ui::theme::SLATE_200))
        .when(!active, |el| el.hover(|s| s.bg(ui::theme::SLATE_100)))
        .child(
            div()
                .size(px(8.))
                .rounded_full()
                .bg(ui::theme::tag_color(tag)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.))
                .text_color(if active {
                    ui::theme::BLACK
                } else {
                    ui::theme::SLATE_600
                })
                .child(name),
        )
        .child(
            div()
                .text_size(px(10.))
                .text_color(ui::theme::SLATE_400)
                .child(count.to_string()),
        )
        .on_click(
            cx.listener(move |this, _e, _w, cx| this.open_quick_filter(QuickFilter::Tag(tag), cx)),
        )
}
fn root_row(state: &AppState, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let active = state.cwd == STORAGE_ROOT;
    h_flex()
        .id("tree-root")
        .a11y_item("Storage root", active)
        .h(px(26.))
        .pr_2()
        .pl(px(4.))
        .gap_1()
        .items_center()
        .rounded_md()
        .cursor_pointer()
        .when(active, |el| el.bg(ui::theme::SLATE_200))
        .when(!active, |el| el.hover(|s| s.bg(ui::theme::SLATE_100)))
        .child(div().w(px(12.)).flex_shrink_0()) // chevron-column alignment
        .child(icon::icon(FOLDER, px(13.), ui::theme::SLATE_500))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.))
                .text_color(if active {
                    ui::theme::BLACK
                } else {
                    ui::theme::SLATE_600
                })
                .child("Internal storage"),
        )
        .on_click(cx.listener(|this, _e, _w, cx| this.navigate(STORAGE_ROOT.to_string(), cx)))
}

/// One tree node: its row plus, when expanded, the indented child level.
fn tree_node(state: &AppState, cx: &mut Context<Workspace>, folder: &Entry, depth: usize) -> Div {
    let path = folder.path.clone();
    let name = folder.name.clone();
    let expanded = state.expanded.contains(&folder.path);
    let has_children = !state.child_folders(&folder.path).is_empty();
    let active = state.cwd == folder.path;
    let row = h_flex()
        .id(gpui_kit::SharedString::from(format!(
            "tree-{}",
            folder.path
        )))
        .a11y_item(name.clone(), active)
        .h(px(26.))
        .pr_2()
        .pl(px(4. + 14. * depth as f32))
        .gap_1()
        .items_center()
        .rounded_md()
        .cursor_pointer()
        .when(active, |el| el.bg(ui::theme::SLATE_200))
        .when(!active, |el| el.hover(|s| s.bg(ui::theme::SLATE_100)))
        .child(chevron(&folder.path, expanded, has_children, cx))
        .child(icon::icon(FOLDER, px(13.), ui::theme::SLATE_400))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.))
                .text_color(if active {
                    ui::theme::BLACK
                } else {
                    ui::theme::SLATE_600
                })
                .child(name),
        )
        .on_click(cx.listener(move |this, _e, _w, cx| this.navigate(path.clone(), cx)));
    v_flex()
        .gap_0p5()
        .child(row)
        .when(expanded && has_children, |el| {
            el.child(tree_children(state, cx, &folder.path, depth + 1))
        })
}

fn tree_children(state: &AppState, cx: &mut Context<Workspace>, parent: &str, depth: usize) -> Div {
    let mut column = v_flex().gap_0p5();
    for folder in state.child_folders(parent) {
        column = column.child(tree_node(state, cx, folder, depth));
    }
    column
}

/// Chevron toggle; empty folders render a spacer cell instead.
fn chevron(
    path: &str,
    expanded: bool,
    has_children: bool,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let toggle = path.to_string();
    if has_children {
        h_flex()
            .id(gpui_kit::SharedString::from(format!("chevron-{path}")))
            .a11y_button(if expanded {
                "Collapse folder"
            } else {
                "Expand folder"
            })
            .w(px(12.))
            .h(px(12.))
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(icon::icon(
                if expanded { CARET_DOWN } else { CARET_RIGHT },
                px(10.),
                ui::theme::SLATE_400,
            ))
            .on_click(cx.listener(move |this, _e, _w, cx| this.toggle_tree_node(&toggle, cx)))
            .into_any_element()
    } else {
        div().w(px(12.)).flex_shrink_0().into_any_element()
    }
}

/// Sidebar footer: §2.5 storage meter above the Wi-Fi/battery readout.
/// Device state and pairing live in the top-bar device chip (⌘L).
fn footer(state: &AppState) -> Div {
    let storage_used = state.storage_used;
    let storage_total = state.device_info.storage_total_bytes;
    let fraction = state.storage_fraction();
    v_flex().mx_2().mb_2().gap_2().child(
        v_flex()
            .px_2()
            .py_1p5()
            .gap_1()
            .rounded_lg()
            .bg(ui::theme::WHITE)
            .border_1()
            .border_color(ui::theme::SLATE_200)
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(ui::theme::SLATE_500)
                            .child(format!(
                                "{} / {}",
                                format_bytes(storage_used, 1),
                                format_bytes(storage_total, 0)
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(ui::theme::SLATE_400)
                            .child(format!("{:.0}%", fraction * 100.)),
                    ),
            )
            .child(ui::progress(fraction, ui::theme::BLACK)),
    )
}
