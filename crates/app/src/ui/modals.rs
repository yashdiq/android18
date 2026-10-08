//! §14 overlays: new-folder + rename modals, toast, error banner.

use crate::ui::A11y as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement, Stateful, StatefulInteractiveElement as _, Styled, Window, div, img, px,
};

use crate::icon::{self, CHECK_CIRCLE, FOLDER_SIMPLE_PLUS, GEAR_SIX, PENCIL_LINE};
use crate::settings::{DefaultView, mask_token};
use crate::state::Toast;
use crate::ui;
use crate::workspace::Workspace;

pub fn new_folder(
    ws: &mut Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let cwd_label = ws.state.display_cwd();
    div()
        .id("new-folder-overlay")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(ui::theme::SLATE_950.opacity(0.4))
        .on_click(cx.listener(|this, _e, _w, cx| this.close_new_folder(cx)))
        .child(
            v_flex()
                .id("new-folder-modal")
                .w(px(360.))
                .gap_3()
                .p_4()
                .bg(ui::theme::WHITE)
                .border_1()
                .border_color(ui::theme::SLATE_200)
                .rounded_xl()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(icon::icon(
                                    FOLDER_SIMPLE_PLUS,
                                    px(16.),
                                    ui::theme::SLATE_600,
                                ))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("New folder"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_family(ui::theme::FONT_MONO)
                                .text_color(ui::theme::SLATE_400)
                                .child(format!("in {cwd_label}")),
                        ),
                )
                .child(Input::new(&ws.new_folder_input).id("new-folder-name"))
                .child(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            h_flex()
                                .id("new-folder-cancel")
                                .h(px(30.))
                                .px_3()
                                .items_center()
                                .rounded_lg()
                                .border_1()
                                .border_color(ui::theme::SLATE_200)
                                .text_size(px(12.))
                                .text_color(ui::theme::SLATE_600)
                                .cursor_pointer()
                                .hover(|s| s.bg(ui::theme::SLATE_50))
                                .child("Cancel")
                                .on_click(
                                    cx.listener(|this, _e, _w, cx| this.close_new_folder(cx)),
                                ),
                        )
                        .child(
                            h_flex()
                                .id("new-folder-create")
                                .h(px(30.))
                                .px_3()
                                .items_center()
                                .rounded_lg()
                                .bg(ui::theme::SLATE_600)
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(ui::theme::WHITE)
                                .cursor_pointer()
                                .hover(|s| s.bg(ui::theme::SLATE_700))
                                .child("Create")
                                .on_click(cx.listener(|this, _e, window, cx| {
                                    this.create_folder(window, cx)
                                })),
                        ),
                ),
        )
        .into_any_element()
}
/// §14.4 rename modal for the anchor entry (R4).
pub fn rename(ws: &mut Workspace, _window: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    let current = ws
        .state
        .selected_entry()
        .map(|entry| entry.name.clone())
        .unwrap_or_default();
    let dir_label = ws.state.display_cwd();
    div()
        .id("rename-overlay")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(ui::theme::SLATE_950.opacity(0.4))
        .on_click(cx.listener(|this, _e, _w, cx| this.close_rename(cx)))
        .child(
            v_flex()
                .id("rename-modal")
                .w(px(360.))
                .gap_3()
                .p_4()
                .bg(ui::theme::WHITE)
                .border_1()
                .border_color(ui::theme::SLATE_200)
                .rounded_xl()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(icon::icon(PENCIL_LINE, px(16.), ui::theme::SLATE_600))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Rename"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_family(ui::theme::FONT_MONO)
                                .text_color(ui::theme::SLATE_400)
                                .child(format!("{current} · in {dir_label}")),
                        ),
                )
                .child(Input::new(&ws.rename_input).id("rename-name"))
                .child(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            h_flex()
                                .id("rename-cancel")
                                .h(px(30.))
                                .px_3()
                                .items_center()
                                .rounded_lg()
                                .border_1()
                                .border_color(ui::theme::SLATE_200)
                                .text_size(px(12.))
                                .text_color(ui::theme::SLATE_600)
                                .cursor_pointer()
                                .hover(|s| s.bg(ui::theme::SLATE_50))
                                .child("Cancel")
                                .on_click(cx.listener(|this, _e, _w, cx| {
                                    this.close_rename(cx);
                                })),
                        )
                        .child(
                            h_flex()
                                .id("rename-confirm")
                                .h(px(30.))
                                .px_3()
                                .items_center()
                                .rounded_lg()
                                .bg(ui::theme::SLATE_600)
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(ui::theme::WHITE)
                                .cursor_pointer()
                                .hover(|s| s.bg(ui::theme::SLATE_700))
                                .child("Rename")
                                .on_click(cx.listener(|this, _e, window, cx| {
                                    this.rename_submit(window, cx)
                                })),
                        ),
                ),
        )
        .into_any_element()
}
pub fn toast(toast: &Toast) -> Stateful<Div> {
    div()
        .id(gpui_kit::SharedString::from(format!("toast-{}", toast.id)))
        .absolute()
        .left_0()
        .right_0()
        .bottom_5()
        .flex()
        .justify_center()
        .child(
            h_flex()
                .h(px(32.))
                .px_3()
                .gap_2()
                .items_center()
                .rounded_full()
                .bg(ui::theme::BLACK)
                .text_color(ui::theme::WHITE)
                .text_size(px(12.))
                .child(icon::icon(CHECK_CIRCLE, px(14.), ui::theme::EMERALD_500))
                .child(div().child(toast.message.clone())),
        )
}

/// §14.4 error banner under the top bar; Escape dismisses.
pub fn error_banner(error: &str) -> Stateful<Div> {
    div()
        .id("error-banner")
        .absolute()
        .left_0()
        .right_0()
        .top(px(64.))
        .flex()
        .justify_center()
        .child(
            h_flex()
                .h(px(32.))
                .px_3()
                .gap_2()
                .items_center()
                .rounded_lg()
                .bg(ui::theme::ROSE_600)
                .text_color(ui::theme::WHITE)
                .text_size(px(12.))
                .child(icon::icon(icon::X, px(14.), ui::theme::WHITE))
                .child(div().child(error.to_string()))
                .child(
                    div()
                        .text_size(px(10.))
                        .opacity(0.7)
                        .child("esc to dismiss"),
                ),
        )
}

/// Native-menu About modal; Escape, the scrim, or OK closes it. Carries the
/// identity mark that used to live in the top bar (§6.1).
pub fn about(ws: &mut Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    let info = &ws.state.device_info;
    let endpoint = match &info.ip_address {
        Some(ip) => format!("{ip}:{}", info.port),
        None => info.base_url.clone(),
    };
    let version = format!("Version {}", env!("CARGO_PKG_VERSION"));
    let app_icon = icon::app_icon();
    div()
        .id("about-overlay")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(ui::theme::SLATE_950.opacity(0.4))
        .on_click(cx.listener(|this, _e, _w, cx| this.toggle_about(cx)))
        .child(
            v_flex()
                .id("about-modal")
                .w(px(320.))
                .gap_3()
                .p_4()
                .bg(ui::theme::WHITE)
                .border_1()
                .border_color(ui::theme::SLATE_200)
                .rounded_xl()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .children(app_icon.map(|mark| img(mark).size(px(28.)).flex_shrink_0()))
                        .child(
                            v_flex()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Android18"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(ui::theme::SLATE_400)
                                        .child(version),
                                ),
                        ),
                )
                .child(
                    v_flex()
                        .pt_2()
                        .border_t_1()
                        .border_color(ui::theme::SLATE_100)
                        .child(ui::meta_row("Device", info.name.clone()))
                        .child(ui::meta_row("Model", info.model.clone()))
                        .child(ui::meta_row("Android", info.android_version.clone()))
                        .child(ui::meta_row("Endpoint", endpoint)),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(ui::theme::SLATE_400)
                        .child("Mock backend — no real device is contacted."),
                )
                .child(
                    h_flex().gap_2().justify_end().child(
                        h_flex()
                            .id("about-ok")
                            .h(px(30.))
                            .px_3()
                            .items_center()
                            .rounded_lg()
                            .bg(ui::theme::SLATE_600)
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(ui::theme::WHITE)
                            .cursor_pointer()
                            .hover(|s| s.bg(ui::theme::SLATE_700))
                            .child("OK")
                            .on_click(cx.listener(|this, _e, _w, cx| this.toggle_about(cx))),
                    ),
                ),
        )
        .into_any_element()
}

/// ⌘, Settings modal (§14 overlay family): persisted default view, locked
/// Light theme, connection/token for the live device, and a two-step
/// forget control. Escape, scrim click, or OK closes.
pub fn settings(ws: &mut Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    let default_view = ws.settings.default_view;
    let forget_armed = ws.forget_armed;
    let live = ws.state.live;
    let info = ws.state.device_info.clone();
    let token_masked = mask_token(&ws.state.token());
    let downloads = crate::download::downloads_root()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "~/Downloads/Android18".into());
    let version = format!("Version {}", env!("CARGO_PKG_VERSION"));
    div()
        .id("settings-overlay")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(ui::theme::SLATE_950.opacity(0.4))
        .on_click(cx.listener(|this, _e, _w, cx| this.close_settings(cx)))
        .child(
            v_flex()
                .id("settings-modal")
                .w(px(360.))
                .gap_3()
                .p_4()
                .bg(ui::theme::WHITE)
                .border_1()
                .border_color(ui::theme::SLATE_200)
                .rounded_xl()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(icon::icon(GEAR_SIX, px(16.), ui::theme::SLATE_600))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Settings"),
                        ),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(ui::label("Appearance"))
                        .child(view_segment(default_view, cx))
                        .child(ui::meta_row("Theme", "Light — dark palette pending".into())),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(ui::label("Connection"))
                        .when(live, |el| {
                            el.child(ui::meta_row("Device", info.name.clone()))
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .items_center()
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .truncate()
                                                .text_size(px(11.))
                                                .font_family(ui::theme::FONT_MONO)
                                                .text_color(ui::theme::SLATE_700)
                                                .child(format!("Token {token_masked}")),
                                        )
                                        .child(
                                            h_flex()
                                                .id("settings-copy-token")
                                                .a11y_button("Copy pairing token")
                                                .h(px(26.))
                                                .px_2()
                                                .items_center()
                                                .rounded_md()
                                                .border_1()
                                                .border_color(ui::theme::SLATE_200)
                                                .text_size(px(11.))
                                                .text_color(ui::theme::SLATE_600)
                                                .cursor_pointer()
                                                .hover(|s| s.bg(ui::theme::SLATE_50))
                                                .child("Copy")
                                                .on_click(cx.listener(|this, _e, _w, cx| {
                                                    this.copy_pairing_token(cx)
                                                })),
                                        ),
                                )
                                .child(forget_button(forget_armed, cx))
                        })
                        .when(!live, |el| {
                            el.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(ui::theme::SLATE_400)
                                    .child("No device connected — pair one from the device chip."),
                            )
                        }),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .border_t_1()
                        .border_color(ui::theme::SLATE_100)
                        .pt_2()
                        .child(ui::label("Downloads"))
                        .child(ui::meta_row("Folder", downloads)),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(ui::theme::SLATE_400)
                                .child(version),
                        )
                        .child(
                            h_flex()
                                .id("settings-ok")
                                .h(px(30.))
                                .px_3()
                                .items_center()
                                .rounded_lg()
                                .bg(ui::theme::SLATE_600)
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(ui::theme::WHITE)
                                .cursor_pointer()
                                .hover(|s| s.bg(ui::theme::SLATE_700))
                                .child("OK")
                                .on_click(cx.listener(|this, _e, _w, cx| this.close_settings(cx))),
                        ),
                ),
        )
        .into_any_element()
}

/// List/Grid segment pair for the launch default, mirroring the §7.1
/// browser toolbar switch; changes apply immediately and persist.
fn view_segment(current: DefaultView, cx: &mut Context<Workspace>) -> Div {
    h_flex()
        .h(px(30.))
        .p_0p5()
        .gap_0p5()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(ui::theme::SLATE_200)
        .bg(ui::theme::SLATE_100)
        .child(segment_button(
            "settings-view-list",
            DefaultView::Table,
            current == DefaultView::Table,
            cx,
        ))
        .child(segment_button(
            "settings-view-grid",
            DefaultView::Grid,
            current == DefaultView::Grid,
            cx,
        ))
}

/// One text segment of [`view_segment`]; the active segment sits on a
/// white pill over the slate track.
fn segment_button(
    id: &'static str,
    target: DefaultView,
    active: bool,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .a11y_item(target.label(), active)
        .flex_1()
        .h(px(26.))
        .items_center()
        .justify_center()
        .rounded_sm()
        .text_size(px(11.))
        .when(active, |el| {
            el.bg(ui::theme::WHITE)
                .border_1()
                .border_color(ui::theme::SLATE_200)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(ui::theme::BLACK)
        })
        .when(!active, |el| {
            el.text_color(ui::theme::SLATE_500)
                .cursor_pointer()
                .hover(|s| s.text_color(ui::theme::SLATE_700))
        })
        .child(target.label())
        .on_click(cx.listener(move |this, _e, _w, cx| this.set_default_view(target, cx)))
}

/// Two-step forget control: the first click arms ("Click again to
/// confirm"), the second deletes the saved token and disconnects.
fn forget_button(forget_armed: bool, cx: &mut Context<Workspace>) -> Stateful<Div> {
    h_flex()
        .id("settings-forget")
        .h(px(28.))
        .items_center()
        .justify_center()
        .rounded_md()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .when(forget_armed, |el| {
            el.bg(ui::theme::BLACK)
                .text_color(ui::theme::WHITE)
                .child("Click again to confirm")
        })
        .when(!forget_armed, |el| {
            el.border_1()
                .border_color(ui::theme::SLATE_200)
                .text_color(ui::theme::SLATE_600)
                .cursor_pointer()
                .hover(|s| s.bg(ui::theme::SLATE_50))
                .child("Forget this device…")
        })
        .on_click(cx.listener(|this, _e, _w, cx| {
            if this.forget_armed {
                this.forget_device(cx);
            } else {
                this.forget_armed = true;
                cx.notify();
            }
        }))
}
