//! §15 server sheet: the connected-phone surface — device identity,
//! security, the live request log, and Disconnect. It only opens while a
//! phone is live; pairing lives on the onboarding gate (`ui::onboarding`),
//! which owns every offline stretch.

use crate::ui::A11y as _;
use android18_core::domain::{Device, HttpLogEntry};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement, Stateful, StatefulInteractiveElement as _, Styled, Window, div, px,
};

use crate::icon::{self, DEVICE_MOBILE, PLUG_CHARGING, SHIELD_CHECK, WIFI_HIGH, X};
use crate::ui::{self, dot, label, meta_row};
use crate::workspace::Workspace;

pub fn modal(ws: &mut Workspace, _window: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    let info = ws.state.device_info.clone();
    let logs = ws.state.device.request_log();
    div()
        .id("server-overlay")
        .absolute()
        .inset_0()
        .flex()
        .justify_end()
        .bg(ui::theme::SLATE_950.opacity(0.3))
        .on_click(cx.listener(|this, _e, _w, cx| this.close_server_sheet(cx)))
        .child(
            v_flex()
                .id("server-sheet")
                .w(px(400.))
                .h_full()
                .bg(ui::theme::WHITE)
                .border_l_1()
                .border_color(ui::theme::SLATE_200)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(live_header(&info, cx))
                .child(
                    v_flex()
                        .id("server-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p_4()
                        .gap_4()
                        .child(details(&info))
                        .child(security_row())
                        .child(disconnect_row(cx))
                        .child(logs_card(&logs)),
                ),
        )
        .into_any_element()
}

/// §15 header, live state: device identity + Connected badge.
fn live_header(info: &Device, cx: &mut Context<Workspace>) -> Div {
    h_flex()
        .p_4()
        .gap_2p5()
        .items_center()
        .border_b_1()
        .border_color(ui::theme::SLATE_100)
        .child(icon::icon(DEVICE_MOBILE, px(20.), ui::theme::SLATE_700))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(info.name.clone()),
                )
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(dot(ui::theme::EMERALD_500))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(ui::theme::EMERALD_700)
                                .child("Connected"),
                        ),
                ),
        )
        .child(
            ui::icon_button("close-server", X, "Close")
                .on_click(cx.listener(|this, _e, _w, cx| this.close_server_sheet(cx))),
        )
}

/// §15.3 identity rows.
fn details(info: &Device) -> Div {
    v_flex()
        .gap_0p5()
        .child(label("Device").pb_1())
        .child(meta_row("Model", info.model.clone()))
        .child(meta_row("Android", info.android_version.clone()))
        .child(meta_row("ID", info.id.clone()))
        .child(transport_row())
        .child(
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
                        .child("Battery"),
                )
                .child(icon::icon(PLUG_CHARGING, px(12.), ui::theme::EMERALD_600))
                .child(
                    div().text_color(ui::theme::EMERALD_700).child(
                        info.battery_percent
                            .map(|b| format!("{b}%"))
                            .unwrap_or_else(|| "—".into()),
                    ),
                ),
        )
        .child(meta_row("Endpoint", info.base_url.clone()))
}

/// Transport meta row — Wi-Fi LAN (USB tunnels ride localhost).
fn transport_row() -> Div {
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
                .child("Transport"),
        )
        .child(icon::icon(WIFI_HIGH, px(12.), ui::theme::EMERALD_600))
        .child(div().text_color(ui::theme::SLATE_700).child("Wi-Fi"))
}

/// §15.4 security row: paired over LAN with the X-Auth token.
fn security_row() -> Div {
    h_flex()
        .gap_1p5()
        .items_center()
        .p_2()
        .rounded_lg()
        .bg(ui::theme::EMERALD_50)
        .child(icon::icon(SHIELD_CHECK, px(14.), ui::theme::EMERALD_600))
        .child(
            div()
                .flex_1()
                .text_size(px(11.))
                .text_color(ui::theme::EMERALD_700)
                .child("Paired over LAN · X-Auth token"),
        )
}

/// Disconnect: drop the live backend and return to the onboarding gate.
fn disconnect_row(cx: &mut Context<Workspace>) -> Stateful<Div> {
    h_flex()
        .id("server-disconnect")
        .a11y_button("Disconnect")
        .h(px(28.))
        .px_3()
        .items_center()
        .justify_center()
        .rounded_lg()
        .border_1()
        .border_color(ui::theme::SLATE_200)
        .text_size(px(11.))
        .text_color(ui::theme::ROSE_600)
        .cursor_pointer()
        .hover(|s| s.bg(ui::theme::SLATE_50))
        .child("Disconnect")
        .on_click(cx.listener(|this, _e, _w, cx| this.disconnect_device(cx)))
}

/// §15.5 the phone's 80-entry request log, newest first.
fn logs_card(logs: &[HttpLogEntry]) -> Div {
    v_flex()
        .gap_1p5()
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .child(dot(ui::theme::EMERALD_500))
                .child(label("Live HTTP log")),
        )
        // The desktop's own heartbeat pings `/info`; hide them so the log
        // shows real traffic.
        .children(
            logs.iter()
                .rev()
                .filter(|entry| !entry.endpoint.starts_with("/info"))
                .take(10)
                .map(log_row)
                .collect::<Vec<_>>(),
        )
}

fn log_row(entry: &HttpLogEntry) -> Div {
    let status_color = match entry.status {
        200..=299 => ui::theme::EMERALD_600,
        400..=499 => ui::theme::AMBER_600,
        _ => ui::theme::ROSE_600,
    };
    h_flex()
        .gap_2()
        .items_center()
        .text_size(px(10.))
        .font_family(ui::theme::FONT_MONO)
        .child(
            div()
                .w(px(30.))
                .flex_shrink_0()
                .text_color(ui::theme::SLATE_500)
                .child(entry.method.as_str().to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(ui::theme::SLATE_700)
                .child(entry.endpoint.clone()),
        )
        .child(
            div()
                .w(px(28.))
                .flex()
                .justify_end()
                .flex_shrink_0()
                .text_color(status_color)
                .child(entry.status.to_string()),
        )
        .child(
            div()
                .w(px(44.))
                .flex()
                .justify_end()
                .flex_shrink_0()
                .text_color(ui::theme::SLATE_400)
                .child(format!("{}ms", entry.duration_ms)),
        )
}
