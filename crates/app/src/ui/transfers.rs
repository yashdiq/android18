//! §12 transfers drawer: bottom-right overlay queue with progress + controls.

use crate::ui::A11y as _;
use android18_core::domain::{TransferDirection, TransferItem, TransferStatus};
use android18_core::transfer::TransferEvent;
use android18_core::util::format::{format_bytes, format_speed};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    Stateful, StatefulInteractiveElement as _, Styled, div, px,
};

use crate::icon::{
    self, ARROW_COUNTER_CLOCKWISE, DOWNLOAD_SIMPLE, PAUSE, PLAY, TRASH, UPLOAD_SIMPLE, X,
};
use crate::ui::{self, DRAWER_W, progress};
use crate::workspace::Workspace;

/// Which lifecycle event a mini control fires.
#[derive(Clone, Copy)]
enum Control {
    Pause,
    Resume,
    Cancel,
}

pub fn drawer(ws: &mut Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    let transfers = ws.state.transfers.clone();
    let active = ws.state.active_transfers();
    v_flex()
        .id("transfers-drawer")
        .absolute()
        .bottom_3()
        .right_3()
        .w(px(DRAWER_W))
        .max_h(px(420.))
        .bg(ui::theme::WHITE)
        .border_1()
        .border_color(ui::theme::SLATE_200)
        .rounded_xl()
        .overflow_hidden()
        .child(
            h_flex()
                .h(px(36.))
                .px_2()
                .gap_2()
                .items_center()
                .flex_shrink_0()
                .border_b_1()
                .border_color(ui::theme::SLATE_100)
                .child(icon::icon(
                    ARROW_COUNTER_CLOCKWISE,
                    px(14.),
                    ui::theme::SLATE_500,
                ))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Transfers"),
                )
                .child(ui::chip(
                    format!("{active} active"),
                    ui::theme::SLATE_700,
                    ui::theme::SLATE_100,
                ))
                .child(div().flex_1())
                // §12 clear-finished: appears once something terminal
                // (completed / cancelled / errored) sits in the queue.
                .when(ws.state.has_finished_transfers(), |el| {
                    el.child(
                        ui::icon_button("clear-finished", TRASH, "Clear finished transfers")
                            .on_click(cx.listener(|this, _e, _w, cx| {
                                this.clear_finished_transfers(cx);
                            })),
                    )
                })
                .child(
                    ui::icon_button("close-transfers", X, "Close")
                        .on_click(cx.listener(|this, _e, _w, cx| this.toggle_transfers(cx))),
                ),
        )
        .child(
            v_flex()
                .id("transfer-list")
                .overflow_y_scroll()
                .when(transfers.is_empty(), |el| {
                    el.child(
                        div()
                            .p_6()
                            .flex()
                            .justify_center()
                            .text_size(px(12.))
                            .text_color(ui::theme::SLATE_400)
                            .child("No transfers yet — download or upload something"),
                    )
                })
                .children(
                    transfers
                        .iter()
                        .map(|item| transfer_row(item, cx))
                        .collect::<Vec<_>>(),
                ),
        )
        .into_any_element()
}
/// §12.2 one queue row: identity, status pill, progress, controls.
fn transfer_row(item: &TransferItem, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let id = item.id;
    let (dir_glyph, dir_tint) = match item.direction {
        TransferDirection::Upload => (UPLOAD_SIMPLE, ui::theme::EMERALD_600),
        TransferDirection::Download => (DOWNLOAD_SIMPLE, ui::theme::SLATE_600),
    };
    let (status_text, status_fg, status_bg) = match item.status {
        TransferStatus::Queued => ("queued", ui::theme::SLATE_600, ui::theme::SLATE_100),
        TransferStatus::InProgress => ("active", ui::theme::SLATE_700, ui::theme::SLATE_100),
        TransferStatus::Paused => ("paused", ui::theme::AMBER_600, ui::theme::AMBER_50),
        TransferStatus::Completed => ("done", ui::theme::EMERALD_700, ui::theme::EMERALD_50),
        TransferStatus::Error | TransferStatus::Cancelled => {
            ("failed", ui::theme::ROSE_600, ui::theme::ROSE_50)
        }
    };
    let pct = (item.progress() * 100.).round() as u32;
    let speed = if item.status == TransferStatus::InProgress && item.speed_bytes_per_sec > 0 {
        format!(" · {}/s", format_speed(item.speed_bytes_per_sec))
    } else {
        String::new()
    };
    let show_pause = item.status == TransferStatus::InProgress;
    let show_resume = item.status == TransferStatus::Paused;
    let show_cancel = !item.status.is_terminal();

    v_flex()
        .id(gpui_kit::SharedString::from(format!("transfer-{id}")))
        .gap_1p5()
        .p_3()
        .border_b_1()
        .border_color(ui::theme::SLATE_100)
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    h_flex()
                        .size(px(22.))
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .bg(dir_tint.opacity(0.12))
                        .child(icon::icon(dir_glyph, px(13.), dir_tint)),
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
                                .child(item.name.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(10.))
                                .font_family(ui::theme::FONT_MONO)
                                .text_color(ui::theme::SLATE_400)
                                .child(item.remote_path.clone()),
                        ),
                )
                .child(ui::chip(status_text, status_fg, status_bg)),
        )
        .child(progress(item.progress(), dir_tint))
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .text_size(px(10.))
                .text_color(ui::theme::SLATE_400)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(ui::theme::FONT_MONO)
                        .child(format!(
                            "{} / {}{speed} · {pct}%",
                            format_bytes(item.transferred, 1),
                            format_bytes(item.size, 1)
                        )),
                )
                .when(show_pause, |el| {
                    el.child(control(id, "pause", PAUSE, Control::Pause, cx))
                })
                .when(show_resume, |el| {
                    el.child(control(id, "resume", PLAY, Control::Resume, cx))
                })
                .when(show_cancel, |el| {
                    el.child(control(id, "cancel", X, Control::Cancel, cx))
                }),
        )
}

/// 20px pause/resume/cancel icon button.
fn control(
    id: u64,
    key: &'static str,
    glyph: &str,
    cmd: Control,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    h_flex()
        .id(gpui_kit::SharedString::from(format!("{key}-{id}")))
        .a11y_button(key.to_string())
        .size(px(20.))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .hover(|s| s.bg(ui::theme::SLATE_100))
        .child(icon::icon(glyph, px(12.), ui::theme::SLATE_500))
        .on_click(cx.listener(move |this, _e, _w, cx| {
            let event = match cmd {
                Control::Pause => TransferEvent::Pause,
                Control::Resume => TransferEvent::Resume,
                Control::Cancel => TransferEvent::Cancel,
            };
            this.transfer_event(id, event, cx);
        }))
}
