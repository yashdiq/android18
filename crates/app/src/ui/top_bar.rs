//! §6.1 top bar: Files↔Dashboard switch on the left (after the native window
//! controls), the ⌘K search launcher and refresh centered, transfers /
//! terminal / device chip on the right. Rooted in the kit
//! `TitleBar`, which owns the drag-to-move region — that is the window-move
//! fix.
//!
//! Overlap contract: the kit reserves 80px of left padding on macOS for the
//! traffic lights and draws its own min/max/close cluster on the right on
//! Windows/Linux. This module never refines horizontal padding on the
//! `TitleBar` itself — a `px_*()` refinement overrides that reserve and puts
//! content under the native buttons. Only interior groups add spacing.

use crate::ui::A11y as _;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::{Icon, Placement, Selectable as _, Sizable as _, TitleBar, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    Stateful, StatefulInteractiveElement as _, Styled, Window, div, px,
};

use crate::icon::{
    self, ARROW_CLOCKWISE, ARROW_DOWN_UP, CARET_DOWN, CHART_PIE, LIST, MAGNIFYING_GLASS,
    TERMINAL_WINDOW,
};
use crate::state::CenterView;
use crate::ui;
use crate::workspace::{Refresh, ToggleDashboard, ToggleTerminal, ToggleTransfers, Workspace};

pub fn render(ws: &mut Workspace, _window: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    let device_name = ws.state.device_info.name.clone();
    let live = ws.state.live;
    let server_sheet_open = ws.state.server_open;
    let active_transfers = ws.state.active_transfers() as u32;
    let terminal_open = ws.state.terminal_open;
    let browser_active = ws.state.view == CenterView::Browser;

    // No `px_*` refinement on purpose — see the overlap contract above.
    TitleBar::new()
        .h(px(ui::TOPBAR_H))
        .bg(ui::theme::WHITE)
        .border_b_1()
        .border_color(ui::theme::SLATE_200)
        .child(
            h_flex()
                .relative()
                .w_full()
                .h_full()
                .items_center()
                .gap_2()
                // Right inset only, never on the TitleBar itself (see the
                // overlap contract above): keeps the device chip off the
                // window edge without touching the traffic-light reserve.
                .pr_2()
                // Left: the Files ↔ Dashboard switch — the first content
                // after the native window controls.
                .child(view_switch(browser_active, cx))
                // Right: transfers and terminal, with the device chip at
                // the far right edge (§15 opens the server sheet).
                .child(
                    h_flex()
                        .ml_auto()
                        .items_center()
                        .gap_1()
                        .child(transfers_button(active_transfers, cx))
                        .child(terminal_button(terminal_open, cx)),
                )
                .child(device_chip(device_name, live, server_sheet_open, cx))
                // Center: ⌘K launcher + refresh, window-centered. The
                // wrapper is non-interactive, so clicks and title-bar
                // dragging fall through everywhere except on its two
                // interactive children.
                .child(
                    h_flex()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(search_launcher(cx))
                        .child(refresh_button(cx)),
                ),
        )
        .into_any_element()
}

/// Files ↔ Dashboard segmented switch (§6.1). Tooltips pin to the bottom so
/// they never clip against the window's top edge / traffic lights.
fn view_switch(browser_active: bool, cx: &mut Context<Workspace>) -> Div {
    h_flex()
        .h(px(30.))
        .p_0p5()
        .gap_0p5()
        .items_center()
        .rounded_lg()
        .bg(ui::theme::SLATE_100)
        .child(
            switch_segment(
                "switch-browser",
                LIST,
                "Files",
                browser_active,
                cx,
                |this, cx| {
                    if this.state.view != CenterView::Browser {
                        this.toggle_center_view(cx);
                    }
                },
            )
            .tooltip_placement(Placement::Bottom),
        )
        .child(
            switch_segment(
                "switch-dashboard",
                CHART_PIE,
                "Dashboard",
                !browser_active,
                cx,
                |this, cx| {
                    if this.state.view != CenterView::Dashboard {
                        this.toggle_center_view(cx);
                    }
                },
            )
            // ⇧⌘D toggles Dashboard specifically, so only this segment
            // advertises the binding.
            .tooltip_with_action("Dashboard", &ToggleDashboard, None)
            .tooltip_placement(Placement::Bottom),
        )
}

/// One segment of the §6.1 switch: the active segment sits on a white pill,
/// the inactive one is transparent over the slate track.
fn switch_segment(
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
        .with_size(px(19.))
        .size(px(26.))
        .rounded(px(6.))
        .tooltip(tip)
        .accessibility_label(tip)
        .on_click(cx.listener(move |this, _e, _w, cx| on_click(this, cx)))
}

/// §12 transfers, with an active-count badge.
fn transfers_button(active: u32, cx: &mut Context<Workspace>) -> Div {
    div()
        .relative()
        .child(
            ui::icon_button("transfers", ARROW_DOWN_UP, "Transfers")
                .tooltip_with_action("Transfers", &ToggleTransfers, None)
                .tooltip_placement(Placement::Bottom)
                .on_click(cx.listener(|this, _e, _w, cx| this.toggle_transfers(cx))),
        )
        .when(active > 0, |el| {
            el.child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .h(px(14.))
                    .min_w(px(14.))
                    .px_0p5()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(ui::theme::SLATE_600)
                    .text_size(px(9.))
                    .text_color(ui::theme::WHITE)
                    .child(active.to_string()),
            )
        })
}

/// §11 terminal toggle.
fn terminal_button(open: bool, cx: &mut Context<Workspace>) -> Button {
    ui::icon_button("terminal", TERMINAL_WINDOW, "Terminal")
        .selected(open)
        .tooltip_with_action("Terminal", &ToggleTerminal, None)
        .tooltip_placement(Placement::Bottom)
        .on_click(cx.listener(|this, _e, _w, cx| this.toggle_terminal(cx)))
}

/// §15 device chip: status dot + name; opens the server sheet (⌘L).
fn device_chip(name: String, live: bool, open: bool, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let dot = if live {
        ui::theme::EMERALD_500
    } else {
        ui::theme::SLATE_400
    };
    h_flex()
        .id("device-chip")
        .a11y_button("Phone connection")
        .h(px(30.))
        .px_2()
        .gap_1p5()
        .items_center()
        .rounded_lg()
        .bg(if open {
            ui::theme::SLATE_100
        } else {
            ui::theme::WHITE
        })
        .border_1()
        .border_color(if open {
            ui::theme::SLATE_300
        } else {
            ui::theme::SLATE_200
        })
        .cursor_pointer()
        .hover(|s| s.border_color(ui::theme::SLATE_300))
        .child(div().size(px(7.)).rounded_full().bg(dot))
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(ui::theme::SLATE_700)
                .truncate()
                .max_w(px(120.))
                .child(name),
        )
        .child(icon::icon(CARET_DOWN, px(10.), ui::theme::SLATE_400))
        .on_click(cx.listener(|this, _e, _w, cx| this.toggle_server_sheet(cx)))
}

/// §13 search launcher.
fn search_launcher(cx: &mut Context<Workspace>) -> Stateful<Div> {
    h_flex()
        .id("search-launcher")
        .a11y_button("Search with AI")
        .w(px(280.))
        .h(px(30.))
        .px_2()
        .gap_2()
        .items_center()
        .rounded_lg()
        .bg(ui::theme::SLATE_50)
        .border_1()
        .border_color(ui::theme::SLATE_200)
        .cursor_pointer()
        .hover(|s| s.border_color(ui::theme::SLATE_300).bg(ui::theme::WHITE))
        .child(icon::icon(MAGNIFYING_GLASS, px(14.), ui::theme::PURPLE_500))
        .child(
            div()
                .flex_1()
                .text_size(px(12.))
                .text_color(ui::theme::SLATE_400)
                .child("Search files or ask AI…"),
        )
        .child(ui::kbd("⌘K"))
        .on_click(cx.listener(|this, _e, window, cx| this.open_search(window, cx)))
}

/// Refresh listing.
fn refresh_button(cx: &mut Context<Workspace>) -> Button {
    ui::icon_button("refresh", ARROW_CLOCKWISE, "Refresh")
        .tooltip_with_action("Refresh", &Refresh, None)
        .tooltip_placement(Placement::Bottom)
        .on_click(cx.listener(|this, _e, _w, cx| this.refresh_list(cx)))
}
