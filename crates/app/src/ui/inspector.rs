//! §9 inspector: metadata, actions and preview for the current selection.

use crate::ui::A11y as _;
use std::sync::Arc;

use android18_core::domain::Entry;
use android18_core::util::categorize::FileCategory;
use android18_core::util::format::{format_bytes, format_date};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, ClipboardItem, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ObjectFit, ParentElement, RenderImage, Stateful, StatefulInteractiveElement as _,
    Styled, StyledImage as _, Window, div, img, px,
};

use crate::icon::{
    self, ARROW_UP_LEFT, COPY, DOWNLOAD_SIMPLE, FILM_STRIP, IMAGE, MUSIC_NOTES, PENCIL_LINE, TRASH,
};
use crate::ui::{self, INSPECTOR_W, label, meta_row};
use crate::workspace::Workspace;

pub fn render(ws: &mut Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    // Two or more picked entries switch the panel to the stacked
    // multi-selection view (§9): summary card, per-entry rows and batch
    // actions over the whole set.
    if ws.state.selected_set.len() >= 2 {
        return multi_view(ws, cx);
    }
    // The shell only mounts the inspector with a live selection.
    let Some(entry) = ws.state.selected_entry().cloned() else {
        return empty_panel().into_any_element();
    };
    let preview = ws
        .state
        .selected_preview
        .clone()
        .or_else(|| entry.content.clone());
    let image = ws.state.selected_preview_image.clone();
    let (glyph, tint) = icon::entry_glyph(&entry);
    let size_text = if entry.dir {
        entry
            .item_count
            .map(|n| format!("{n} items"))
            .unwrap_or_else(|| "—".into())
    } else if entry.size > 0 {
        format!("{} ({} B)", format_bytes(entry.size, 1), entry.size)
    } else {
        format_bytes(entry.size, 1)
    };

    v_flex()
        .id("inspector")
        .w(px(INSPECTOR_W))
        .flex_shrink_0()
        .h_full()
        .min_h_0()
        .border_l_1()
        .border_color(ui::theme::SLATE_200)
        .bg(ui::theme::WHITE)
        // §9 header: drawer title + close (prototype parity).
        .child(
            h_flex()
                .p_3()
                .gap_2()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(ui::theme::SLATE_100)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .min_w_0()
                        .child(icon::icon(glyph, px(16.), tint))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(ui::theme::BLACK)
                                .child("Inspector & Details"),
                        ),
                )
                .child(
                    ui::icon_button("close-inspector", icon::X, "Close")
                        .on_click(cx.listener(|this, _e, _w, cx| this.toggle_inspector(cx))),
                ),
        )
        .child(
            v_flex()
                .id("inspector-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_2()
                .gap_2()
                // §9.3 preview card first, full width (files only).
                .when(!entry.dir, |el| {
                    el.child(preview_block(&entry, preview.as_deref(), image.as_ref()))
                })
                // §9 storage path + copy affordance.
                .child(path_card(&entry.path, cx))
                // §9.1 metadata.
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(label("Details").pb_1())
                        .child(meta_row("Type", FileCategory::of(&entry).to_string()))
                        .child(meta_row("Size", size_text))
                        .child(meta_row("Modified", format_date(entry.mtime)))
                        .child(meta_row(
                            "Kind",
                            entry.mime_type.clone().unwrap_or_else(|| "—".into()),
                        ))
                        .when_some(entry.color_tag, |el, tag| {
                            el.child(
                                h_flex()
                                    .gap_2()
                                    .py_1()
                                    .items_center()
                                    .text_size(px(11.))
                                    .child(
                                        div()
                                            .w(px(72.))
                                            .flex_shrink_0()
                                            .text_color(ui::theme::SLATE_400)
                                            .child("Tag"),
                                    )
                                    .child(ui::chip(
                                        tag_label(tag),
                                        ui::theme::WHITE,
                                        ui::theme::tag_color(tag),
                                    )),
                            )
                        }),
                )
                // §9.2 actions: Download (files) + Rename, Cut + Delete.
                .child(action_grid(&entry, cx)),
        )
        .into_any_element()
}
/// §9 multi-selection view (two or more picked entries): a summary card
/// with a stacked-cards preview, per-entry rows (capped at 50 — the panel
/// is a summary, not a second list view), and batch actions that act on
/// the whole selection set.
fn multi_view(ws: &mut Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    let summary = ws.state.selection_summary();
    let entries = ws.state.selection_entries();
    let overflow = summary.count.saturating_sub(entries.len().min(50));
    let shown: Vec<Entry> = entries.iter().take(50).cloned().collect();

    v_flex()
        .id("inspector-multi")
        .w(px(INSPECTOR_W))
        .flex_shrink_0()
        .h_full()
        .min_h_0()
        .border_l_1()
        .border_color(ui::theme::SLATE_200)
        .bg(ui::theme::WHITE)
        // §9 header, same as the single view.
        .child(
            h_flex()
                .p_3()
                .gap_2()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(ui::theme::SLATE_100)
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ui::theme::BLACK)
                        .child("Inspector & Details"),
                )
                .child(
                    ui::icon_button("close-inspector", icon::X, "Close")
                        .on_click(cx.listener(|this, _e, _w, cx| this.toggle_inspector(cx))),
                ),
        )
        .child(
            v_flex()
                .id("inspector-multi-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_3()
                .gap_3()
                // §9 summary: stacked-cards preview + count chip.
                .child(stack_card(&shown, summary.count))
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(label("Details").pb_1())
                        .child(meta_row("Items", summary.count.to_string()))
                        .child(meta_row("Folders", summary.folders.to_string()))
                        .child(meta_row("Files", summary.files.to_string()))
                        .child(meta_row("Total size", format_bytes(summary.bytes, 1))),
                )
                .when(overflow > 0, |el| {
                    el.child(
                        div()
                            .text_size(px(11.))
                            .text_color(ui::theme::SLATE_400)
                            .child(format!("+{overflow} more not shown")),
                    )
                })
                // §9.2 batch actions over the whole set.
                .child(
                    v_flex()
                        .gap_2()
                        .child(h_flex().gap_2().child(action_button(
                            "multi-download",
                            DOWNLOAD_SIMPLE,
                            "Download",
                            ActionStyle::Primary,
                            cx,
                            |this, _window, cx| this.download_selected(cx),
                        )))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(action_button(
                                    "multi-cut",
                                    ARROW_UP_LEFT,
                                    "Move (Cut)",
                                    ActionStyle::Outline,
                                    cx,
                                    |this, _window, cx| this.cut_selected(cx),
                                ))
                                .child(action_button(
                                    "multi-delete",
                                    TRASH,
                                    "Delete",
                                    ActionStyle::Danger,
                                    cx,
                                    |this, _window, cx| this.delete_selected(cx),
                                )),
                        ),
                )
                // Per-entry rows in listing order.
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(label("Selected").pb_1())
                        .children(shown.iter().map(entry_row)),
                ),
        )
        .into_any_element()
}
/// Stacked-cards preview: up to three offset card backs behind the top
/// entry's glyph — a pile of picked files, not a second hero card.
fn stack_card(shown: &[Entry], count: usize) -> Div {
    let layers = shown.len().min(3);
    let (glyph, tint) = shown
        .first()
        .map(icon::entry_glyph)
        .unwrap_or((icon::FILE, ui::theme::SLATE_400));
    v_flex()
        .relative()
        .items_center()
        .gap_2()
        .p_4()
        .rounded_xl()
        .bg(ui::theme::SLATE_50)
        .border_1()
        .border_color(ui::theme::SLATE_100)
        .child(
            div()
                .relative()
                .size(px(64.))
                // Card backs behind the top card, deepest painted first.
                .children((1..layers).rev().map(|i| {
                    div()
                        .absolute()
                        .top(px(3. * i as f32))
                        .left(px(3. * i as f32))
                        .size(px(56.))
                        .rounded_lg()
                        .bg(ui::theme::WHITE)
                        .border_1()
                        .border_color(ui::theme::SLATE_200)
                }))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size(px(56.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_lg()
                        .bg(ui::theme::WHITE)
                        .border_1()
                        .border_color(ui::theme::SLATE_200)
                        .shadow_sm()
                        .child(icon::icon(glyph, px(26.), tint)),
                ),
        )
        .child(ui::chip(
            format!(
                "{} item{} selected",
                count,
                if count == 1 { "" } else { "s" }
            ),
            ui::theme::SLATE_700,
            ui::theme::SLATE_100,
        ))
}

/// One compact row of the multi-selection list: glyph chip, name +
/// category, size on the right.
fn entry_row(entry: &Entry) -> Div {
    let (glyph, tint) = icon::entry_glyph(entry);
    let size = if entry.dir {
        entry
            .item_count
            .map(|n| format!("{n} items"))
            .unwrap_or_else(|| "Folder".into())
    } else {
        format_bytes(entry.size, 1)
    };
    h_flex()
        .gap_2()
        .items_center()
        .p_1p5()
        .rounded_lg()
        .bg(ui::theme::SLATE_50)
        .border_1()
        .border_color(ui::theme::SLATE_100)
        .child(
            h_flex()
                .size(px(26.))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .rounded_md()
                .bg(tint.opacity(0.12))
                .child(icon::icon(glyph, px(14.), tint)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(ui::theme::SLATE_800)
                        .child(entry.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(ui::theme::SLATE_400)
                        .child(FileCategory::of(entry).to_string()),
                ),
        )
        .child(
            div()
                .text_size(px(10.))
                .font_family(ui::theme::FONT_MONO)
                .text_color(ui::theme::SLATE_400)
                .child(size),
        )
}
/// §9 storage path block with a copy affordance; the toast confirms.
fn path_card(path: &str, cx: &mut Context<Workspace>) -> Div {
    let target = path.to_string();
    v_flex()
        .gap_1p5()
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(label("Storage path"))
                .child(
                    h_flex()
                        .id("inspector-copy-path")
                        .a11y_button("Copy path")
                        .gap_1()
                        .items_center()
                        .cursor_pointer()
                        .text_size(px(11.))
                        .text_color(ui::theme::SLATE_600)
                        .hover(|s| s.text_color(ui::theme::BLACK))
                        .child(icon::icon(COPY, px(12.), ui::theme::SLATE_500))
                        .child("Copy")
                        .on_click(cx.listener(move |this, _e, _w, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(target.clone()));
                            this.show_toast("Path copied to clipboard", cx);
                        })),
                ),
        )
        .child(
            div()
                .w_full()
                .p_2()
                .rounded_lg()
                .bg(ui::theme::SLATE_50)
                .border_1()
                .border_color(ui::theme::SLATE_100)
                .font_family(ui::theme::FONT_MONO)
                .text_size(px(11.))
                .text_color(ui::theme::SLATE_800)
                .child(path.to_string()),
        )
}

/// §9.2 action grid: Download (files) + Rename on the first row, Move To +
/// Delete on the second. Click handlers receive the window from their
/// listeners, so modals focus their inputs cleanly.
fn action_grid(entry: &Entry, cx: &mut Context<Workspace>) -> AnyElement {
    let first_row = if entry.dir {
        h_flex().gap_2().child(action_button(
            "inspect-rename",
            PENCIL_LINE,
            "Rename",
            ActionStyle::Outline,
            cx,
            |this, window, cx| this.open_rename(window, cx),
        ))
    } else {
        h_flex()
            .gap_2()
            .child(action_button(
                "inspect-download",
                DOWNLOAD_SIMPLE,
                "Download",
                ActionStyle::Primary,
                cx,
                |this, _window, cx| this.download_selected(cx),
            ))
            .child(action_button(
                "inspect-rename",
                PENCIL_LINE,
                "Rename",
                ActionStyle::Outline,
                cx,
                |this, window, cx| this.open_rename(window, cx),
            ))
    };
    v_flex()
        .gap_2()
        .child(first_row)
        .child(
            h_flex()
                .gap_2()
                .child(action_button(
                    "inspect-cut",
                    ARROW_UP_LEFT,
                    "Cut",
                    ActionStyle::Outline,
                    cx,
                    |this, _window, cx| this.cut_selected(cx),
                ))
                .child(action_button(
                    "inspect-delete",
                    TRASH,
                    "Delete",
                    ActionStyle::Danger,
                    cx,
                    |this, _window, cx| this.delete_selected(cx),
                )),
        )
        .into_any_element()
}

/// Visual variant for one §9.2 action button.
enum ActionStyle {
    Primary,
    Outline,
    Danger,
}

/// One §9.2 action button: icon + label, styled by variant.
fn action_button(
    id: &'static str,
    glyph: &str,
    text: &'static str,
    style: ActionStyle,
    cx: &mut Context<Workspace>,
    on_click: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) -> Stateful<Div> {
    let (bg, hover, fg) = match style {
        ActionStyle::Primary => (ui::theme::SLATE_600, ui::theme::SLATE_700, ui::theme::WHITE),
        ActionStyle::Outline => (ui::theme::WHITE, ui::theme::SLATE_50, ui::theme::SLATE_600),
        ActionStyle::Danger => (ui::theme::WHITE, ui::theme::ROSE_50, ui::theme::ROSE_600),
    };
    h_flex()
        .id(id)
        .a11y_button(text)
        .h(px(30.))
        .flex_1()
        .gap_1p5()
        .items_center()
        .justify_center()
        .rounded_lg()
        .border_1()
        .border_color(ui::theme::SLATE_200)
        .bg(bg)
        .text_color(fg)
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(icon::icon(glyph, px(14.), fg))
        .child(div().child(text))
        .on_click(cx.listener(move |this, _e, window, cx| on_click(this, window, cx)))
}

/// §9.3 preview, by category: a fetched image/video poster (live devices),
/// a category icon panel for other media, the first 12 text lines, or a
/// fallback card.
fn preview_block(entry: &Entry, preview: Option<&str>, image: Option<&Arc<RenderImage>>) -> Div {
    let media = |glyph: &str, tint: Hsla| {
        div()
            .w_full()
            .aspect_ratio(4. / 3.)
            .flex()
            .flex_col()
            .gap_2()
            .items_center()
            .justify_center()
            .rounded_xl()
            .bg(ui::theme::SLATE_100)
            .child(icon::icon(glyph, px(28.), tint))
            .into_any_element()
    };
    let body = if let Some(rendered) = image {
        // Fetched poster: the card takes the frame's own aspect ratio (capped
        // for tall pages), so the image fills it with no letterbox bands.
        let size = rendered.size(0);
        let ratio = (size.width.0 as f32 / size.height.0.max(1) as f32).max(0.625);
        div()
            .w_full()
            .aspect_ratio(ratio)
            .overflow_hidden()
            .rounded_xl()
            .border_1()
            .border_color(ui::theme::SLATE_200)
            .bg(ui::theme::SLATE_100)
            .child(
                img(rendered.clone())
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element()
    } else {
        match FileCategory::of(entry) {
            FileCategory::Image | FileCategory::BinaryImage => media(IMAGE, ui::theme::SLATE_300),
            FileCategory::Video => media(FILM_STRIP, ui::theme::PURPLE_500),
            FileCategory::Audio => media(MUSIC_NOTES, ui::theme::SKY_500),
            _ => {
                if let Some(text) = preview {
                    v_flex()
                        .gap_0p5()
                        .p_2()
                        .rounded_lg()
                        .bg(ui::theme::SLATE_50)
                        .border_1()
                        .border_color(ui::theme::SLATE_100)
                        .font_family(ui::theme::FONT_MONO)
                        .children(
                            text.lines()
                                .take(12)
                                .map(|line| {
                                    div()
                                        .text_size(px(11.))
                                        .text_color(ui::theme::SLATE_700)
                                        .child(line.to_string())
                                })
                                .collect::<Vec<_>>(),
                        )
                        .into_any_element()
                } else {
                    let (glyph, tint) = icon::entry_glyph(entry);
                    div()
                        .w_full()
                        .aspect_ratio(4. / 3.)
                        .flex()
                        .flex_col()
                        .gap_2()
                        .items_center()
                        .justify_center()
                        .rounded_xl()
                        .bg(ui::theme::SLATE_100)
                        .child(icon::icon(glyph, px(32.), tint))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(ui::theme::SLATE_400)
                                .child("No preview available"),
                        )
                        .into_any_element()
                }
            }
        }
    };
    v_flex().child(body)
}

/// §2.4 tag chip captions.
fn tag_label(tag: android18_core::domain::ColorTag) -> &'static str {
    use android18_core::domain::ColorTag;
    match tag {
        ColorTag::Blue => "Blue",
        ColorTag::Emerald => "Emerald",
        ColorTag::Amber => "Amber",
        ColorTag::Purple => "Purple",
        ColorTag::Rose => "Rose",
        ColorTag::Slate => "Slate",
    }
}

/// Shown if the inspector ever renders without a selection.
fn empty_panel() -> Stateful<Div> {
    v_flex()
        .id("inspector-empty")
        .w(px(INSPECTOR_W))
        .flex_shrink_0()
        .h_full()
        .border_l_1()
        .border_color(ui::theme::SLATE_200)
        .bg(ui::theme::WHITE)
        .child(
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .child(icon::icon(icon::FILE, px(28.), ui::theme::SLATE_300))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(ui::theme::SLATE_400)
                        .child("Nothing selected"),
                ),
        )
}
