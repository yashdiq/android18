//! §11 terminal overlay: dark slate panel with transcript + prompt input.

use android18_core::shell::OutputKind;
use gpui_kit::base::input::Input as BaseInput;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, Hsla, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, div, px,
};

use crate::icon::{self, ARROW_IN_SIMPLE, ARROW_OUT_SIMPLE, SPARKLE, TERMINAL_WINDOW, X};
use crate::ui::{self, TERMINAL_H};
use crate::workspace::Workspace;

fn kind_color(kind: OutputKind) -> Hsla {
    match kind {
        OutputKind::Output => ui::theme::SLATE_200,
        OutputKind::Cmd => ui::theme::SLATE_500,
        OutputKind::Error => ui::theme::ROSE_500,
        OutputKind::Ai => ui::theme::PURPLE_300,
        OutputKind::Success => ui::theme::EMERALD_500,
    }
}

/// Dark-surface variant of the icon button (hover in slate-800).
fn dark_button(id: &'static str, glyph: &str, tip: &str, cx: &App) -> Button {
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .foreground(ui::theme::SLATE_400)
                .hover(ui::theme::SLATE_800)
                .active(ui::theme::SLATE_700),
        )
        .icon(Icon::empty().path(glyph))
        .with_size(px(18.))
        .size(px(24.))
        .rounded(px(6.))
        .tooltip(tip)
        .accessibility_label(tip)
}

pub fn overlay(ws: &mut Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    let prompt = ws
        .shell_prompt()
        .unwrap_or_else(|| "android18@pixel-9-pro:~$ ".into());
    let maximized = ws.state.terminal_maximized;

    let mut panel = v_flex()
        .id("terminal")
        // Key context for ↑/↓ history recall: the bindings in `main.rs`
        // match "Terminal > Input" so they outrank the base input's own
        // MoveUp/MoveDown without touching any other input in the app.
        .key_context("Terminal")
        .absolute()
        .bg(ui::theme::SLATE_950)
        .border_1()
        .border_color(ui::theme::SLATE_800)
        .rounded_xl()
        .overflow_hidden();
    if maximized {
        panel = panel.inset_2();
    } else {
        panel = panel.left_3().right_3().bottom_3().h(px(TERMINAL_H));
    }

    panel
        // §11 header.
        .child(
            h_flex()
                .h(px(36.))
                .px_2()
                .gap_2()
                .items_center()
                .flex_shrink_0()
                .border_b_1()
                .border_color(ui::theme::SLATE_800)
                .child(icon::icon(TERMINAL_WINDOW, px(14.), ui::theme::SLATE_300))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_family(ui::theme::FONT_MONO)
                        .text_color(ui::theme::SLATE_400)
                        .child("android18@pixel-9-pro"),
                )
                .child(div().flex_1())
                .child(
                    h_flex()
                        .h(px(18.))
                        .px_1p5()
                        .gap_1()
                        .items_center()
                        .rounded_md()
                        .bg(ui::theme::PURPLE_950)
                        .border_1()
                        .border_color(ui::theme::PURPLE_800)
                        .child(icon::icon(SPARKLE, px(10.), ui::theme::PURPLE_300))
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(ui::theme::PURPLE_300)
                                .child("ai <question>"),
                        ),
                )
                .child(
                    dark_button(
                        "term-max",
                        if maximized {
                            ARROW_IN_SIMPLE
                        } else {
                            ARROW_OUT_SIMPLE
                        },
                        if maximized { "Restore" } else { "Maximize" },
                        cx,
                    )
                    .on_click(cx.listener(|this, _e, _w, cx| this.toggle_terminal_max(cx))),
                )
                .child(
                    dark_button("term-close", X, "Close", cx)
                        .on_click(cx.listener(|this, _e, _w, cx| this.toggle_terminal(cx))),
                ),
        )
        // §11.2 transcript.
        .child(
            v_flex()
                .id("term-transcript")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_3()
                .gap_0p5()
                .font_family(ui::theme::FONT_MONO)
                .text_size(px(12.))
                .children(
                    ws.state
                        .transcript
                        .iter()
                        .map(|line| {
                            div()
                                .text_color(kind_color(line.kind))
                                .child(line.text.clone())
                        })
                        .collect::<Vec<_>>(),
                ),
        )
        // §11.3 prompt.
        .child(
            h_flex()
                .p_2()
                .gap_2()
                .items_center()
                .flex_shrink_0()
                .border_t_1()
                .border_color(ui::theme::SLATE_800)
                .child(
                    div()
                        .text_size(px(11.))
                        .font_family(ui::theme::FONT_MONO)
                        .text_color(ui::theme::EMERALD_500)
                        .child(prompt),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(ui::theme::FONT_MONO)
                        .text_size(px(12.))
                        // Typed glyphs paint with the inherited text colour
                        // (the root is black), so set it for the dark panel.
                        .text_color(ui::theme::SLATE_200)
                        // Base (unstyled) input: the kit component would
                        // re-project light-theme ink over this dark panel
                        // every frame. Inks come from the one-time
                        // `set_editor_style` at construction; font and size
                        // inherit from this wrapper.
                        .child(BaseInput::new(&ws.terminal_input)),
                ),
        )
        .into_any_element()
}
