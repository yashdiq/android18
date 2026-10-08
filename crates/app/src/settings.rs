//! Persisted app settings, stored as pretty JSON at
//! `~/Library/Application Support/Android18/settings.json`.
//!
//! Pure data + (de)serialization only: the workspace loads once at launch
//! and writes changes through a background task
//! ([`crate::workspace::Workspace::persist_settings`]).

use std::path::Path;
use std::path::PathBuf;

use android18_core::domain::ViewMode;
use serde::{Deserialize, Serialize};

/// Which view the browser opens in. Mirrors `ViewMode` without dragging
/// serde into the core domain type.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefaultView {
    #[default]
    Table,
    Grid,
}

impl DefaultView {
    /// Segment label (§7.1 calls the table view "List").
    pub fn label(self) -> &'static str {
        match self {
            DefaultView::Table => "List",
            DefaultView::Grid => "Grid",
        }
    }
}

impl From<DefaultView> for ViewMode {
    fn from(view: DefaultView) -> Self {
        match view {
            DefaultView::Table => ViewMode::Table,
            DefaultView::Grid => ViewMode::Grid,
        }
    }
}

/// UI theme. Only Light exists today; the persisted field keeps a future
/// dark palette a value change instead of a schema migration.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    Light,
}

/// Smallest window the app supports (mirrors `window_min_size` in `main.rs`).
pub const MIN_WINDOW: (i32, i32) = (980, 620);

/// Last window frame, in whole logical pixels so `AppSettings` stays
/// `Copy + Eq`. `maximized` keeps the restore frame in the other fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    #[serde(default)]
    pub maximized: bool,
}

impl WindowState {
    /// Rejects frames that are corrupt or smaller than the minimum size.
    pub fn validated(self) -> Option<Self> {
        (self.width >= MIN_WINDOW.0 && self.height >= MIN_WINDOW.1).then_some(self)
    }
}

/// App settings. Unknown fields (newer build) and missing fields (older
/// build) both fall back to defaults, so no settings file can break launch.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub default_view: DefaultView,
    pub theme: ThemeChoice,
    pub window: Option<WindowState>,
}

/// `~/Library/Application Support/Android18/settings.json` (`None` when
/// `$HOME` is unset).
pub fn settings_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Android18")
            .join("settings.json")
    })
}

impl AppSettings {
    /// Parses settings JSON; a corrupt file falls back to defaults.
    pub fn from_json(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_default()
    }

    /// Serializes for the settings file. Infallible for this plain-data
    /// struct, so the invariant panics loudly instead of losing settings.
    pub fn to_json(self) -> String {
        serde_json::to_string_pretty(&self).expect("settings serialize")
    }

    /// Reads the settings file, defaulting on any error (missing file,
    /// unreadable, corrupt JSON).
    pub fn load() -> Self {
        settings_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| Self::from_json(&text))
            .unwrap_or_default()
    }

    /// Writes to `path`, creating parent directories. `Err` travels to the
    /// caller (the workspace drops it — settings are cosmetic).
    pub fn write_to(self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_json())
    }
}

/// Masked rendering for the Settings token row: short tokens never leak,
/// longer ones keep a recognizable head/tail.
pub fn mask_token(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 8 {
        "••••".to_string()
    } else {
        let head: String = chars[..3].iter().collect();
        let tail: String = chars[chars.len() - 3..].iter().collect();
        format!("{head}••••{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let settings = AppSettings {
            default_view: DefaultView::Grid,
            theme: ThemeChoice::Light,
            window: Some(WindowState {
                x: 10,
                y: 20,
                width: 1100,
                height: 700,
                maximized: true,
            }),
        };
        assert_eq!(AppSettings::from_json(&settings.to_json()), settings);
    }

    #[test]
    fn old_settings_without_window_load() {
        let settings = AppSettings::from_json(r#"{"default_view":"grid","theme":"light"}"#);
        assert_eq!(settings.window, None);
        assert_eq!(settings.default_view, DefaultView::Grid);
    }

    #[test]
    fn window_state_rejects_undersized_frames() {
        let ok = WindowState {
            x: 0,
            y: 0,
            width: 980,
            height: 620,
            maximized: false,
        };
        assert_eq!(ok.validated(), Some(ok));
        assert_eq!(WindowState { width: 979, ..ok }.validated(), None);
        assert_eq!(WindowState { height: 0, ..ok }.validated(), None);
    }

    #[test]
    fn defaults_for_missing_fields_and_corrupt_files() {
        assert_eq!(AppSettings::from_json("{}"), AppSettings::default());
        assert_eq!(AppSettings::from_json("{{{"), AppSettings::default());
        // Unknown fields from a newer build are ignored.
        assert_eq!(
            AppSettings::from_json(r#"{"default_view":"grid","future":true}"#),
            AppSettings {
                default_view: DefaultView::Grid,
                ..AppSettings::default()
            }
        );
    }

    #[test]
    fn default_view_maps_to_domain_view_mode() {
        assert_eq!(ViewMode::from(DefaultView::Table), ViewMode::Table);
        assert_eq!(ViewMode::from(DefaultView::Grid), ViewMode::Grid);
        assert_eq!(DefaultView::Table.label(), "List");
        assert_eq!(DefaultView::Grid.label(), "Grid");
    }

    #[test]
    fn token_masking() {
        assert_eq!(mask_token(""), "••••");
        assert_eq!(mask_token("short"), "••••");
        assert_eq!(mask_token("12345678"), "••••");
        assert_eq!(mask_token("abcdef0123456"), "abc••••456");
    }

    #[test]
    fn write_creates_parents_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("android18-settings-{}", std::process::id()));
        let path = dir.join("nested/settings.json");
        let settings = AppSettings {
            default_view: DefaultView::Grid,
            ..AppSettings::default()
        };
        settings.write_to(&path).expect("write settings");
        let text = std::fs::read_to_string(&path).expect("read settings back");
        assert_eq!(AppSettings::from_json(&text), settings);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
