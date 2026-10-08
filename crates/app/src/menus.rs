//! Native menu bar, registered from `main` via `App::set_menus`.
//!
//! macOS renders the first menu as the bold application menu. Every item
//! dispatches a workspace action, so shortcut glyphs come from the §16
//! keymap and the handlers are the same ones the keybindings use — no
//! parallel dispatch path. Menus are static: live checkmarks would need a
//! re-`set_menus` on every state change.

use gpui_kit::{Menu, MenuItem, SystemMenuType};

use crate::workspace::{
    About, CycleSort, Minimize, Quit, Refresh, Settings, ToggleDashboard, ToggleFullscreen,
    ToggleInspector, ToggleSearch, ToggleServer, ToggleTerminal, ToggleTransfers, ToggleViewMode,
    Zoom,
};

pub fn app_menus() -> Vec<Menu> {
    vec![
        Menu::new("Android18").items(vec![
            MenuItem::action("About Android18", About),
            MenuItem::separator(),
            MenuItem::action("Settings…", Settings),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Quit Android18", Quit),
        ]),
        Menu::new("View").items(vec![
            MenuItem::action("Search", ToggleSearch),
            MenuItem::action("Phone Connection…", ToggleServer),
            MenuItem::action("Toggle View", ToggleDashboard),
            MenuItem::action("Grid/Table", ToggleViewMode),
            MenuItem::action("Sort By", CycleSort),
            MenuItem::separator(),
            MenuItem::action("Inspector", ToggleInspector),
            MenuItem::action("Terminal", ToggleTerminal),
            MenuItem::action("Transfers", ToggleTransfers),
            MenuItem::separator(),
            MenuItem::action("Refresh", Refresh),
        ]),
        Menu::new("Window").items(vec![
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
            MenuItem::action("Toggle Full Screen", ToggleFullscreen),
        ]),
        Menu::new("Help").items(vec![MenuItem::action("About Android18", About)]),
    ]
}
