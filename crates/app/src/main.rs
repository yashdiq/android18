mod assets;
mod download;
mod icon;
mod menus;
mod platform;
mod settings;
mod state;
mod theme;
mod thumb;
mod ui;
mod upload;
mod workspace;

use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;
use workspace::{
    CopySelected, CutSelected, CycleSort, Escape, Minimize, NavigateBack, NavigateForward,
    NavigateUp, PasteIntoFolder, Quit, Refresh, Settings, TerminalHistoryNext, TerminalHistoryPrev,
    ToggleDashboard, ToggleFullscreen, ToggleInspector, ToggleSearch, ToggleServer, ToggleTerminal,
    ToggleTransfers, ToggleViewMode,
};

fn main() {
    let app = gpui_kit::application().with_assets(assets::AppAssets);
    // Dock-icon click with every window closed reopens the window.
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_main_window(cx);
        }
        cx.activate(true);
    });
    app.run(|cx| {
        assets::load_fonts(cx);
        gpui_kit::init(cx);
        // Dock/app icon before the first window shows (no-op off macOS).
        platform::set_app_icon();
        Theme::change(ThemeMode::Light, None, cx);

        // §16 keymap.
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-up", NavigateUp, None),
            KeyBinding::new("cmd-[", NavigateBack, None),
            KeyBinding::new("cmd-]", NavigateForward, None),
            KeyBinding::new("cmd-r", Refresh, None),
            KeyBinding::new("cmd-g", ToggleViewMode, None),
            KeyBinding::new("cmd-shift-s", CycleSort, None),
            KeyBinding::new("cmd-k", ToggleSearch, None),
            KeyBinding::new("cmd-l", ToggleServer, None),
            KeyBinding::new("cmd-shift-d", ToggleDashboard, None),
            KeyBinding::new("cmd-`", ToggleTerminal, None),
            KeyBinding::new("cmd-shift-t", ToggleTransfers, None),
            KeyBinding::new("cmd-shift-p", ToggleServer, None),
            KeyBinding::new("cmd-i", ToggleInspector, None),
            KeyBinding::new("escape", Escape, None),
            // §7.1 cut/copy/paste (context-free: inside a focused
            // input the kit's own Input-context copy/cut bindings win).
            KeyBinding::new("cmd-c", CopySelected, None),
            KeyBinding::new("cmd-x", CutSelected, None),
            KeyBinding::new("cmd-v", PasteIntoFolder, None),
            // §11.3 history recall: ↑/↓ inside the terminal prompt.
            // The "Terminal > Input" descendant predicate matches at
            // the deepest depth and (registered later) outranks the
            // base input's own up/down bindings — only there.
            KeyBinding::new("up", TerminalHistoryPrev, Some("Terminal > Input")),
            KeyBinding::new("down", TerminalHistoryNext, Some("Terminal > Input")),
            // Menu-bar companions (Window menu + app Settings item).
            KeyBinding::new("cmd-,", Settings, None),
            KeyBinding::new("cmd-m", Minimize, None),
            KeyBinding::new("ctrl-cmd-f", ToggleFullscreen, None),
        ]);

        // Native macOS menu bar; items dispatch the same actions the
        // keymap serves (see `menus.rs`).
        cx.set_menus(menus::app_menus());

        // Dock right-click menu (same actions as the menu bar).
        cx.set_dock_menu(vec![
            MenuItem::action("Transfers", ToggleTransfers),
            MenuItem::action("Connect Phone…", ToggleServer),
            MenuItem::action("Refresh", Refresh),
        ]);

        // Quit when the last window closes. GPUI keeps the process alive
        // otherwise, and windowless its menu bar is unusable — menu-item
        // validation and action dispatch both need an active window — so the
        // app would linger with every item (incl. Quit) disabled (B12).
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        // Global Quit fallback: runs only when no window handler claimed the
        // action, keeping ⌘Q and the menu item live in any residual state.
        cx.on_action::<Quit>(|_, cx| cx.quit());

        open_main_window(cx);
    });
}

/// Builds the window options (restoring the saved frame) and opens the
/// main window; also used by the Dock-reopen handler.
fn open_main_window(cx: &mut App) {
    // `TitleBar::window_options` installs the custom titlebar with
    // `app_owns_titlebar_drag`, so the kit TitleBar owns window moving.
    // The traffic lights move down to the vertical center of the 56px
    // bar (§5): macOS buttons are 14px tall, so y = (56 - 14) / 2 = 21
    // (the kit default of 9 targets its shorter TitleBar).
    let titlebar = TitlebarOptions {
        traffic_light_position: Some(point(px(9.), px(21.))),
        ..TitleBar::title_bar_options()
    };
    // Restore the last frame (validated; default frame otherwise).
    let saved = settings::AppSettings::load()
        .window
        .and_then(settings::WindowState::validated);
    let window_bounds = match saved {
        Some(w) => {
            let bounds = Bounds {
                origin: point(px(w.x as f32), px(w.y as f32)),
                size: size(px(w.width as f32), px(w.height as f32)),
            };
            if w.maximized {
                WindowBounds::Maximized(bounds)
            } else {
                WindowBounds::Windowed(bounds)
            }
        }
        None => WindowBounds::Windowed(Bounds {
            origin: point(px(140.), px(90.)),
            size: size(px(1280.), px(820.)),
        }),
    };
    let options = WindowOptions {
        window_bounds: Some(window_bounds),
        window_min_size: Some(size(px(980.), px(620.))),
        titlebar: Some(titlebar),
        ..TitleBar::window_options()
    };
    gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| workspace::Workspace::new(window, cx))
    })
    .expect("failed to open the Android18 window");
}
