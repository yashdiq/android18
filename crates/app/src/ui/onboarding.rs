//! §15 onboarding gate: the full-screen "Connect your phone" surface shown
//! whenever no phone is live — first run, a manual disconnect, or a dropped
//! connection. It replaces the whole window; the file shell renders only
//! once a phone answers. Every pairing path lives here (moved out of the
//! server sheet in B11): plug in over USB and allow, scan the QR from the
//! phone app, connect a discovered phone, or use the advanced address form.
//! A successful connect swaps the gate for the shell (`apply_live_backend`);
//! a lost connection returns here (`go_offline`) with a fresh pairing
//! session and device watch.

use crate::ui::A11y as _;
use android18_core::util::qr;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement, StatefulInteractiveElement as _, Styled, Window, div, px,
};

use crate::icon::{self, CHECK_CIRCLE, DEVICE_MOBILE, MAGNIFYING_GLASS, PLUG_CHARGING};
use crate::state::{DeviceCandidate, PairingSession};
use crate::ui::{self, dot, label};
use crate::workspace::Workspace;

/// On-screen QR size (modules + 4-module quiet zone). 300px keeps the
/// compact payload's modules ~8px wide — far above the scanner floor.
const QR_SIDE_PX: f32 = 300.;
/// Width of each column of the two-pane gate body.
const PANE_W: f32 = 420.;

pub fn render(ws: &mut Workspace, _window: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    v_flex()
        .id("onboarding")
        .size_full()
        .bg(ui::theme::SLATE_50)
        .child(
            // Rooted in the kit TitleBar for the drag-to-move region; no
            // `px_*` refinement on it (traffic-light overlap contract, §6.1).
            TitleBar::new()
                .h(px(ui::TOPBAR_H))
                .bg(ui::theme::SLATE_50)
                .border_b_1()
                .border_color(ui::theme::SLATE_200)
                .child(
                    // Brand-free bar: just the connection state, centered
                    // clear of the traffic lights.
                    h_flex()
                        .w_full()
                        .h_full()
                        .justify_center()
                        .gap_1p5()
                        .child(dot(ui::theme::SLATE_300))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(ui::theme::SLATE_500)
                                .child("Not connected"),
                        ),
                ),
        )
        .child(
            // Two-pane body: pairing paths on the left, the QR on the right.
            // It scrolls as a whole so narrow windows keep both reachable;
            // the panes row rides auto margins, so it centers in the window
            // when it fits and degrades to the scroll origin (top-left)
            // when it doesn't.
            h_flex()
                .id("onboarding-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .items_start()
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .m_auto()
                        .gap_8()
                        .p_8()
                        .child(
                            v_flex()
                                .id("onboarding-left")
                                .w(px(PANE_W))
                                .flex_shrink_0()
                                .gap_4()
                                .child(hero())
                                .child(usb_section(ws))
                                .child(discovered_section(ws, cx))
                                .child(manual_section(ws, cx)),
                        )
                        .child(
                            v_flex()
                                .id("onboarding-right")
                                .w(px(PANE_W))
                                .flex_shrink_0()
                                .gap_2()
                                .child(label("Scan from your phone"))
                                .child(h_flex().justify_center().child(
                                    match ws.state.pairing_session.as_ref() {
                                        Some(session) => qr_card(session),
                                        None => waiting_card(),
                                    },
                                ))
                                .child(session_meta(
                                    ws.state.pairing_session.as_ref(),
                                    &ws.state.pairing_status,
                                ))
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(ui::theme::SLATE_400)
                                        .child(
                                            "Open the app on your phone and tap Scan desktop QR. \
                                             Same Wi-Fi network.",
                                        ),
                                ),
                        ),
                ),
        )
        .into_any_element()
}

/// Login hero: device mark, title, and the one-paragraph pitch.
fn hero() -> Div {
    v_flex()
        .gap_2()
        .child(icon::icon(DEVICE_MOBILE, px(36.), ui::theme::SLATE_400))
        .child(
            div()
                .text_size(px(24.))
                .font_weight(FontWeight::BOLD)
                .text_color(ui::theme::SLATE_800)
                .child("Connect your phone"),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(ui::theme::SLATE_500)
                .child(
                    "Browse a phone's storage over its built-in HTTP service. Plug \
                     in over USB, scan this QR from the phone app, or connect a \
                     discovered device — the workspace opens as soon as a phone \
                     answers.",
                ),
        )
}

/// USB line. Plugging in raises the allow prompt on the phone by itself;
/// this only reflects what the watcher sees.
fn usb_section(ws: &Workspace) -> Div {
    let usb = ws
        .state
        .candidates
        .iter()
        .find(|candidate| candidate.adb_serial.is_some());
    let line = match usb {
        Some(candidate) if ws.state.connecting => {
            format!("{} — tap Allow on the phone", candidate.name)
        }
        Some(candidate) => format!("{} detected over USB", candidate.name),
        None => "Plug in via USB — the phone will ask you to allow this computer".to_string(),
    };
    h_flex()
        .gap_2()
        .items_center()
        .p_2()
        .rounded_lg()
        .bg(ui::theme::WHITE)
        .border_1()
        .border_color(ui::theme::SLATE_200)
        .child(icon::icon(PLUG_CHARGING, px(14.), ui::theme::SLATE_500))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(11.))
                .text_color(ui::theme::SLATE_600)
                .child(line),
        )
}

/// Discovered devices (mDNS + adb) with Connect affordances.
fn discovered_section(ws: &mut Workspace, cx: &mut Context<Workspace>) -> Div {
    let scan = ws.state.device_scan.clone();
    let candidates = ws.state.candidates.clone();
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(label("Discovered devices"))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(10.))
                        .text_color(ui::theme::SLATE_400)
                        .text_right()
                        .truncate()
                        .child(scan),
                )
                .child(
                    h_flex()
                        .id("device-rescan")
                        .a11y_button("Rescan for devices")
                        .h(px(22.))
                        .px_2()
                        .items_center()
                        .gap_1()
                        .rounded_lg()
                        .border_1()
                        .border_color(ui::theme::SLATE_200)
                        .text_size(px(10.))
                        .text_color(ui::theme::SLATE_600)
                        .cursor_pointer()
                        .hover(|s| s.bg(ui::theme::SLATE_100))
                        .child(icon::icon(MAGNIFYING_GLASS, px(10.), ui::theme::SLATE_500))
                        .child("Scan")
                        .on_click(cx.listener(|this, _e, _w, cx| this.scan_devices(cx))),
                ),
        )
        .children({
            let code_target = ws.state.code_target.clone();
            let code_input = ws.device_code_input.clone();
            candidates
                .iter()
                .map(|candidate| {
                    let entering = code_target.as_deref() == Some(candidate.id.as_str());
                    candidate_row(candidate, entering.then_some(&code_input), cx)
                })
                .collect::<Vec<_>>()
        })
}

/// Collapsed "Advanced" — address + optional code, for networks where
/// mDNS is blocked.
fn manual_section(ws: &mut Workspace, cx: &mut Context<Workspace>) -> Div {
    let open = ws.state.pairing_advanced;
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .id("advanced-toggle")
                .a11y_button("Advanced: connect by address")
                .gap_1()
                .items_center()
                .cursor_pointer()
                .child(label("Advanced: connect by address"))
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(ui::theme::SLATE_400)
                        .child(if open { "Hide" } else { "Show" }),
                )
                .on_click(cx.listener(|this, _e, _w, cx| {
                    this.state.pairing_advanced = !this.state.pairing_advanced;
                    cx.notify();
                })),
        )
        .when(open, |el| {
            el.child(Input::new(&ws.device_url_input).id("device-url"))
                .child(Input::new(&ws.manual_code_input).id("device-code"))
                .child(
                    h_flex()
                        .id("device-connect")
                        .a11y_button("Connect")
                        .h(px(28.))
                        .px_3()
                        .items_center()
                        .justify_center()
                        .rounded_lg()
                        .bg(ui::theme::BLACK)
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ui::theme::WHITE)
                        .cursor_pointer()
                        .hover(|s| s.bg(ui::theme::SLATE_700))
                        .child("Connect")
                        .on_click(cx.listener(|this, _e, _w, cx| this.connect_manual(cx))),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(ui::theme::SLATE_400)
                        .child(
                            "Leave the code empty — the phone will ask you to allow this computer.",
                        ),
                )
        })
}

/// One discovered-device row. Connect rides the saved pairing token when one
/// exists; otherwise a phone that advertises a code gets an inline 6-char
/// input, and any other phone is asked to allow this computer. `code_input`
/// is `Some` while this row is collecting the code.
fn candidate_row(
    candidate: &DeviceCandidate,
    code_input: Option<&Entity<InputState>>,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let needs_code = candidate.code_pairing && !candidate.paired;
    let row = h_flex()
        .id(gpui_kit::SharedString::from(format!(
            "device-row-{}",
            candidate.id
        )))
        .h(px(40.))
        .px_2()
        .gap_2()
        .items_center()
        .rounded_lg()
        .bg(ui::theme::WHITE)
        .border_1()
        .border_color(ui::theme::SLATE_200)
        .child(icon::icon(DEVICE_MOBILE, px(14.), ui::theme::SLATE_500))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(candidate.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .font_family(ui::theme::FONT_MONO)
                        .text_color(ui::theme::SLATE_400)
                        .truncate()
                        .child(if candidate.base_url.is_empty() {
                            format!("{} · no tunnel", candidate.source.label())
                        } else {
                            format!("{} · {}", candidate.source.label(), candidate.base_url)
                        }),
                ),
        )
        .when(candidate.paired, |el| {
            el.child(icon::icon(CHECK_CIRCLE, px(12.), ui::theme::EMERALD_600))
        })
        .child({
            let candidate = candidate.clone();
            h_flex()
                .id(gpui_kit::SharedString::from(format!(
                    "device-connect-{}",
                    candidate.id
                )))
                .a11y_button(format!("Connect to {}", candidate.name))
                .h(px(26.))
                .px_2()
                .items_center()
                .rounded_lg()
                .bg(ui::theme::BLACK)
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(ui::theme::WHITE)
                .cursor_pointer()
                .hover(|s| s.bg(ui::theme::SLATE_700))
                .child(match (candidate.paired, needs_code) {
                    (true, _) => "Connect",
                    (false, true) => "Enter code",
                    (false, false) => "Allow",
                })
                .on_click(cx.listener(move |this, _e, window, cx| {
                    if needs_code {
                        this.open_code_entry(&candidate.id, window, cx);
                    } else {
                        this.connect_candidate(&candidate, cx);
                    }
                }))
        });
    match code_input {
        None => row.into_any_element(),
        Some(input) => v_flex()
            .gap_1()
            .child(row)
            .child(
                v_flex()
                    .gap_1()
                    .pl_2()
                    .child(Input::new(input).id("device-code-inline"))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(ui::theme::SLATE_400)
                            .child("Type the 6-character code from the phone's Server screen."),
                    ),
            )
            .into_any_element(),
    }
}

/// Status dot + line under the QR, the endpoint in mono, and the
/// 6-char pair code.
fn session_meta(session: Option<&PairingSession>, status: &str) -> Div {
    let (endpoint, code) = match session {
        Some(session) => (format!("http://{}", session.endpoint), session.code.clone()),
        None => (String::new(), "——————".into()),
    };
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .child(div().size(px(6.)).rounded_full().bg(ui::theme::AMBER_500))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(ui::theme::SLATE_600)
                        .child(status.to_string()),
                ),
        )
        .child(
            h_flex().justify_center().w_full().child(
                div()
                    .text_size(px(11.))
                    .font_family(ui::theme::FONT_MONO)
                    .text_color(ui::theme::SLATE_400)
                    .child(endpoint),
            ),
        )
        .child(
            h_flex()
                .gap_1()
                .justify_center()
                .w_full()
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(ui::theme::SLATE_400)
                        .child("Pair code"),
                )
                .child(
                    div()
                        .text_size(px(20.))
                        .font_family(ui::theme::FONT_MONO)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ui::theme::SLATE_700)
                        .child(code),
                ),
        )
}

// ---------------------------------------------------------------------
// QR plumbing (compact `a18|2|ip|port|code` payload → QR v3 → 300px).
// ---------------------------------------------------------------------

/// The QR square: white card, run-length-rendered modules with the
/// built-in 4-module quiet zone so the phone scanner locks on.
fn qr_card(session: &PairingSession) -> Div {
    let payload = pairing_payload(session);
    div()
        .p_3()
        .bg(ui::theme::WHITE)
        .border_1()
        .border_color(ui::theme::SLATE_200)
        .rounded_lg()
        .child(match qr::encode(payload.as_bytes()) {
            Ok(matrix) => qr_grid(&matrix),
            // The pairing payload is fixed-shape and far below capacity,
            // so this is a defensive fallback, not an expected path.
            Err(e) => div()
                .w(px(QR_SIDE_PX))
                .h(px(QR_SIDE_PX))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .text_color(ui::theme::SLATE_400)
                .child(e.to_string()),
        })
}

/// The QR payload. Compact pipe format — `a18|2|<ip>|<port>|<code>` —
/// encodes as QR v3 (29 modules), which scans far faster than the old
/// ~64-byte JSON (v5). `endpoint` is a `SocketAddr`'s `ip:port` and
/// `code` is 6 base32 chars, so the fixed-shape string is always
/// well-formed.
fn pairing_payload(session: &PairingSession) -> String {
    match session.endpoint.rsplit_once(':') {
        Some((ip, port)) => format!("a18|2|{ip}|{port}|{}", session.code),
        None => format!("a18|2|0.0.0.0|0|{}", session.code),
    }
}

/// Renders the matrix as one row-div per module row; equal-color runs
/// collapse into single divs, keeping the element count small.
fn qr_grid(matrix: &qr::QrMatrix) -> Div {
    const QUIET: usize = 4;
    let cells = matrix.size() + QUIET * 2;
    let module = (QR_SIDE_PX / cells as f32).floor().max(2.);
    let side = module * cells as f32;
    let mut rows = v_flex()
        .w(px(side))
        .h(px(side))
        .bg(ui::theme::WHITE)
        .px(px(module * QUIET as f32))
        .py(px(module * QUIET as f32));
    for row in 0..matrix.size() {
        let mut runs: Vec<Div> = Vec::new();
        let mut col = 0;
        while col < matrix.size() {
            let dark = matrix.is_dark(row, col);
            let mut len = 1;
            while col + len < matrix.size() && matrix.is_dark(row, col + len) == dark {
                len += 1;
            }
            let run = div().w(px(module * len as f32)).h(px(module));
            runs.push(if dark { run.bg(ui::theme::BLACK) } else { run });
            col += len;
        }
        rows = rows.child(h_flex().children(runs));
    }
    rows
}

/// Placeholder while the listener binds (or failed to bind).
fn waiting_card() -> Div {
    div()
        .w(px(QR_SIDE_PX + 24.))
        .h(px(QR_SIDE_PX + 24.))
        .flex()
        .items_center()
        .justify_center()
        .bg(ui::theme::WHITE)
        .border_1()
        .border_color(ui::theme::SLATE_200)
        .rounded_lg()
        .child(icon::icon(DEVICE_MOBILE, px(40.), ui::theme::SLATE_300))
}

#[cfg(test)]
mod tests {
    use super::pairing_payload;
    use crate::state::PairingSession;

    fn session(endpoint: &str, code: &str) -> PairingSession {
        PairingSession {
            endpoint: endpoint.into(),
            code: code.into(),
        }
    }

    #[test]
    fn payload_splits_endpoint_into_ip_and_port() {
        assert_eq!(
            pairing_payload(&session("192.168.1.42:51820", "0X1D8C")),
            "a18|2|192.168.1.42|51820|0X1D8C"
        );
    }

    #[test]
    fn payload_keeps_fixed_shape_without_a_port() {
        assert_eq!(
            pairing_payload(&session("not-a-socket", "ABC234")),
            "a18|2|0.0.0.0|0|ABC234"
        );
    }
}
