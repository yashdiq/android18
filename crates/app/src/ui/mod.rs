//! §6 shell surfaces, one module each, composed from the shared primitives
//! below plus the [`crate::theme`] tokens — no component hardcodes a color.

pub mod browser;
pub mod dashboard;
pub mod folder_tree;
pub mod inspector;
pub mod modals;
pub mod onboarding;
pub mod search;
pub mod server_sheet;
pub mod terminal;
pub mod top_bar;
pub mod transfers;

pub(crate) use crate::theme;

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _, Size, h_flex};
use gpui_kit::{
    App, Div, ElementId, FontWeight, Hsla, ParentElement, SharedString, Styled, div, px, relative,
};

/// Fixed "now" for relative-time rendering (the mock timeline is autumn 2026).
pub const NOW_MS: i64 = 1_799_200_000_000;

// §5 shell metrics (pixels).
pub const TOPBAR_H: f32 = 56.;
pub const SIDEBAR_W: f32 = 256.;
pub const INSPECTOR_W: f32 = 320.;
pub const TERMINAL_H: f32 = 384.;
pub const DRAWER_W: f32 = 384.;

/// §4 caption: 11px semibold uppercase section label.
pub fn label(text: &str) -> Div {
    div()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::SLATE_400)
        .child(text.to_uppercase())
}

/// 28×28 ghost icon button with tooltip and accessible name; callers attach
/// `.on_click` and may override the plain tooltip with `tooltip_with_action`
/// to surface a bound keyboard shortcut.
pub fn icon_button(id: impl Into<ElementId>, glyph: &str, tip: &str) -> Button {
    Button::new(id)
        .ghost()
        .icon(Icon::empty().path(glyph))
        .tooltip(tip)
        .accessibility_label(tip)
        .size(px(28.))
        .rounded(px(6.))
        .text_color(theme::SLATE_500)
}

/// 26×26 primary icon button (slate-900 fill, white 14px glyph) — §7.1 New
/// Folder: the prototype's `p-1.5` around a `w-3.5` glyph with `rounded-lg`
/// and `shadow-2xs` (`Size::Small` icons render at 14px).
pub fn primary_icon_button(id: impl Into<ElementId>, glyph: &str, tip: &str, cx: &App) -> Button {
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(theme::SLATE_900)
                .foreground(theme::WHITE)
                .hover(theme::SLATE_800)
                .active(theme::SLATE_700),
        )
        .text_color(theme::WHITE)
        .icon(Icon::empty().path(glyph))
        .with_size(Size::Small)
        .size(px(26.))
        .rounded(theme::RADIUS_LG)
        .shadow_2xs()
        .tooltip(tip)
        .accessibility_label(tip)
}

/// 26×26 outline icon button (white fill, slate border, slate-600 glyph) —
/// §7.1 Upload: the prototype's `p-1.5` around a `w-3.5` glyph with
/// `rounded-lg` and `shadow-2xs`.
pub fn outline_icon_button(id: impl Into<ElementId>, glyph: &str, tip: &str, cx: &App) -> Button {
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(theme::WHITE)
                .hover(theme::SLATE_50)
                .active(theme::SLATE_100),
        )
        .text_color(theme::SLATE_600)
        .icon(Icon::empty().path(glyph))
        .with_size(Size::Small)
        .size(px(26.))
        .rounded(theme::RADIUS_LG)
        .shadow_2xs()
        .border_1()
        .border_color(theme::SLATE_200)
        .tooltip(tip)
        .accessibility_label(tip)
}

/// Small status pill (§2.2 accent tints).
pub fn chip(text: impl Into<SharedString>, fg: Hsla, bg: Hsla) -> Div {
    h_flex()
        .h(px(18.))
        .px_1p5()
        .items_center()
        .rounded_md()
        .bg(bg)
        .text_color(fg)
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

/// Keyboard-cap hint (⌘K, esc…).
pub fn kbd(text: &str) -> Div {
    h_flex()
        .h(px(18.))
        .min_w(px(18.))
        .px_1()
        .items_center()
        .justify_center()
        .rounded_sm()
        .bg(theme::SLATE_50)
        .border_1()
        .border_color(theme::SLATE_200)
        .font_family(theme::FONT_MONO)
        .text_size(px(10.))
        .text_color(theme::SLATE_500)
        .child(text.to_string())
}

/// §7.4 progress track with a filled fraction.
pub fn progress(fraction: f32, color: Hsla) -> Div {
    div()
        .h_1()
        .w_full()
        .rounded_full()
        .bg(theme::SLATE_100)
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(color)
                .w(relative(fraction.clamp(0., 1.))),
        )
}

/// 6px status dot.
pub fn dot(color: Hsla) -> Div {
    div().size(px(6.)).rounded_full().bg(color)
}

/// White bordered card used across dashboard/inspector/overlays.
pub fn card() -> Div {
    div()
        .bg(theme::WHITE)
        .border_1()
        .border_color(theme::SLATE_200)
        .rounded_xl()
        .p_3()
}

/// Label/value row for the inspector and server sheet.
pub fn meta_row(label_text: &str, value: String) -> Div {
    h_flex()
        .gap_2()
        .py_1()
        .text_size(px(11.))
        .child(
            div()
                .w(px(72.))
                .flex_shrink_0()
                .text_color(theme::SLATE_400)
                .child(label_text.to_string()),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_color(theme::SLATE_700)
                .child(value),
        )
}

/// Accessibility helpers for clickable `div`s (VoiceOver role + name).
pub trait A11y: gpui_kit::StatefulInteractiveElement + Sized {
    /// Chrome control: button role, spoken name, reachable with Tab.
    fn a11y_button(self, label: impl Into<SharedString>) -> Self {
        self.role(gpui_kit::Role::Button)
            .aria_label(label)
            .tab_index(0)
    }

    /// List/grid item: button role, spoken name and selection state. Not a
    /// tab stop (long listings would make Tab unusable).
    fn a11y_item(self, label: impl Into<SharedString>, selected: bool) -> Self {
        self.role(gpui_kit::Role::Button)
            .aria_label(label)
            .aria_selected(selected)
    }
}

impl<T: gpui_kit::StatefulInteractiveElement> A11y for T {}
