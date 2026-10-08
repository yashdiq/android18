//! §13 ⌘K modal: AI search input with explained matches.

use crate::ui::A11y as _;
use android18_core::fs::paths::parent_of;
use android18_core::search::{Confidence, SearchMatch, SearchResult};
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement, Stateful, StatefulInteractiveElement as _, Styled, div, px,
};

use crate::icon::{self, FILE, SPARKLE};
use crate::ui::{self, chip, kbd, label};
use crate::workspace::Workspace;

pub fn modal(ws: &mut Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    let result = ws.state.search.clone();
    let loading = ws.state.search_loading;
    div()
        .id("search-overlay")
        .absolute()
        .inset_0()
        .flex()
        .justify_center()
        .pt(px(120.))
        .bg(ui::theme::SLATE_950.opacity(0.4))
        .on_click(cx.listener(|this, _e, _w, cx| this.close_search(cx)))
        .child(
            v_flex()
                .id("search-modal")
                .w(px(560.))
                .max_h(px(420.))
                .bg(ui::theme::WHITE)
                .border_1()
                .border_color(ui::theme::SLATE_200)
                .rounded_xl()
                .overflow_hidden()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                // §13.2 input row.
                .child(
                    h_flex()
                        .p_3()
                        .gap_2()
                        .items_center()
                        .border_b_1()
                        .border_color(ui::theme::SLATE_100)
                        .child(icon::icon(SPARKLE, px(16.), ui::theme::PURPLE_500))
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&ws.search_input).id("modal-search")),
                        )
                        .child(kbd("esc")),
                )
                // Results / hints.
                .child(
                    v_flex()
                        .id("search-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(match result.as_ref() {
                            _ if loading => v_flex()
                                .p_3()
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(ui::theme::SLATE_500)
                                        .child("Searching…"),
                                )
                                .into_any_element(),
                            None => hints().into_any_element(),
                            Some(result) => results(result, cx).into_any_element(),
                        }),
                ),
        )
        .into_any_element()
}

/// §13.4 example queries before the first run.
fn hints() -> Div {
    v_flex()
        .p_3()
        .gap_2()
        .child(label("Try asking"))
        .child(
            v_flex().gap_1().children(
                [
                    "camera photos",
                    "large files",
                    "notes from October",
                    "APK installs",
                ]
                .iter()
                .map(|hint| {
                    h_flex()
                        .gap_2()
                        .px_2()
                        .h(px(28.))
                        .items_center()
                        .rounded_md()
                        .bg(ui::theme::SLATE_50)
                        .child(icon::icon(SPARKLE, px(12.), ui::theme::PURPLE_300))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(ui::theme::SLATE_600)
                                .child(hint.to_string()),
                        )
                })
                .collect::<Vec<_>>(),
            ),
        )
        .child(
            div()
                .px_1()
                .text_size(px(10.))
                .text_color(ui::theme::SLATE_400)
                .child("Press enter to search the whole device"),
        )
}
/// Summary line plus every explained match.
fn results(result: &SearchResult, cx: &mut Context<Workspace>) -> Div {
    v_flex()
        .p_3()
        .gap_2()
        .child(
            h_flex()
                .gap_1p5()
                .items_start()
                .child(icon::icon(SPARKLE, px(12.), ui::theme::PURPLE_500))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(12.))
                        .text_color(ui::theme::SLATE_600)
                        .child(result.summary.clone()),
                ),
        )
        .when_some(result.warning.clone(), |el, warning| {
            el.child(
                div()
                    .text_size(px(11.))
                    .text_color(ui::theme::AMBER_600)
                    .child(warning),
            )
        })
        .when_some(result.engine.clone(), |el, engine| {
            el.child(
                div()
                    .text_size(px(10.))
                    .text_color(ui::theme::SLATE_400)
                    .child(format!("engine: {engine}")),
            )
        })
        .child(
            v_flex().gap_1().children(
                result
                    .matches
                    .iter()
                    .map(|m| match_row(m, cx))
                    .collect::<Vec<_>>(),
            ),
        )
}

/// One hit: basename, reason, confidence pill; clicking reveals the file.
fn match_row(m: &SearchMatch, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let path = m.path.clone();
    let parent = parent_of(&m.path);
    let (fg, bg) = match m.confidence {
        Confidence::High => (ui::theme::EMERALD_700, ui::theme::EMERALD_50),
        Confidence::Medium => (ui::theme::AMBER_600, ui::theme::AMBER_50),
        Confidence::Low => (ui::theme::SLATE_500, ui::theme::SLATE_100),
    };
    h_flex()
        .id(gpui_kit::SharedString::from(format!("hit-{}", m.path)))
        .a11y_button(format!("Open {}", m.path))
        .gap_2()
        .px_2()
        .h(px(36.))
        .items_center()
        .rounded_md()
        .cursor_pointer()
        .hover(|s| s.bg(ui::theme::SLATE_50))
        .child(icon::icon(FILE, px(14.), ui::theme::SLATE_400))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(ui::theme::SLATE_800)
                        .child(basename(&m.path).to_string()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(10.))
                        .text_color(ui::theme::SLATE_400)
                        .child(m.reason.clone()),
                ),
        )
        .child(chip(m.confidence.as_str(), fg, bg))
        .on_click(cx.listener(move |this, _e, _w, cx| {
            if let Some(parent) = parent.clone() {
                this.navigate(parent, cx);
            }
            this.select_entry(Some(path.clone()), cx);
            this.close_search(cx);
        }))
}

/// Last non-empty path segment, without panicking on odd paths.
fn basename(path: &str) -> &str {
    path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path)
}
