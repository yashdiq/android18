//! Phosphor duotone icon registry (DESIGN.md §3 + Appendix B).
//!
//! Every glyph is a vendored `icons/duotone/<name>-duotone.svg`. The duotone secondary
//! layer is baked at 20% opacity, so a single `text_color` tint styles both
//! layers (§3.2). Icons are never rendered as emoji or text glyphs (§16.1).
//!
//! The registry mirrors Appendix B in full: every asset path stays defined
//! even before a surface first uses it, so batch landings don't churn it.
#![allow(dead_code)]

use std::sync::{Arc, OnceLock};

use android18_core::domain::Entry;
use android18_core::util::categorize::FileCategory;
use gpui_kit::{Hsla, Pixels, RenderImage, Styled, Svg, svg};

use crate::theme;

// --- Asset paths (Appendix B). ---------------------------------------------
pub const ANDROID_LOGO: &str = "icons/duotone/android-logo-duotone.svg";
pub const ARROW_UP_LEFT: &str = "icons/regular/arrow-up-left.svg";
pub const ARROW_CLOCKWISE: &str = "icons/regular/arrow-clockwise.svg";
pub const ARROW_COUNTER_CLOCKWISE: &str = "icons/regular/arrows-counter-clockwise.svg";
pub const ARROW_DOWN_UP: &str = "icons/regular/arrows-down-up.svg";
pub const ARROW_IN_SIMPLE: &str = "icons/regular/arrows-in-simple.svg";
pub const ARROW_OUT_SIMPLE: &str = "icons/regular/arrows-out-simple.svg";
pub const ARROW_U_UP_LEFT: &str = "icons/regular/arrow-u-up-left.svg";
pub const ARROW_U_UP_RIGHT: &str = "icons/regular/arrow-u-up-right.svg";
pub const ARROW_UP: &str = "icons/regular/arrow-up.svg";
pub const CAMERA: &str = "icons/duotone/camera-duotone.svg";
pub const CARET_DOWN: &str = "icons/regular/caret-down.svg";
pub const CARET_LEFT: &str = "icons/regular/caret-left.svg";
pub const CARET_RIGHT: &str = "icons/regular/caret-right.svg";
pub const CARET_UP: &str = "icons/regular/caret-up.svg";
pub const CHART_PIE: &str = "icons/regular/chart-pie.svg";
pub const CHAT_CIRCLE: &str = "icons/duotone/chat-circle-duotone.svg";
pub const CHECK: &str = "icons/duotone/check-duotone.svg";
pub const CHECK_CIRCLE: &str = "icons/duotone/check-circle-duotone.svg";
pub const CLOCK: &str = "icons/duotone/clock-duotone.svg";
pub const CLOCK_CLOCKWISE: &str = "icons/regular/clock-clockwise.svg";
pub const COPY: &str = "icons/regular/copy-simple.svg";
pub const CPU: &str = "icons/duotone/cpu-duotone.svg";
pub const DEVICE_MOBILE: &str = "icons/duotone/device-mobile-duotone.svg";
pub const DOWNLOAD_SIMPLE: &str = "icons/regular/download-simple.svg";
pub const EYE: &str = "icons/regular/eye.svg";
pub const FILE: &str = "icons/duotone/file-duotone.svg";
pub const FILE_CODE: &str = "icons/duotone/file-code-duotone.svg";
pub const FILE_TEXT: &str = "icons/duotone/file-text-duotone.svg";
pub const FILE_ZIP: &str = "icons/duotone/file-zip-duotone.svg";
pub const FILM_STRIP: &str = "icons/duotone/film-strip-duotone.svg";
pub const PENCIL_LINE: &str = "icons/regular/pencil-simple-line.svg";
pub const FOLDER: &str = "icons/duotone/folder-duotone.svg";
pub const FOLDER_OPEN: &str = "icons/regular/folder-open.svg";
pub const PLUS: &str = "icons/regular/plus.svg";
pub const FOLDER_SIMPLE_PLUS: &str = "icons/regular/folder-simple-plus.svg";
pub const FUNNEL_SIMPLE: &str = "icons/regular/funnel-simple.svg";
pub const GEAR_SIX: &str = "icons/duotone/gear-six-duotone.svg";
pub const HARD_DRIVES: &str = "icons/duotone/hard-drives-duotone.svg";
pub const IMAGE: &str = "icons/duotone/image-duotone.svg";
pub const LIST: &str = "icons/regular/list.svg";
pub const MAGNIFYING_GLASS: &str = "icons/regular/magnifying-glass.svg";
pub const MUSIC_NOTES: &str = "icons/duotone/music-notes-duotone.svg";
pub const PAUSE: &str = "icons/duotone/pause-duotone.svg";
pub const PATH: &str = "icons/duotone/path-duotone.svg";
pub const PLAY: &str = "icons/duotone/play-duotone.svg";
pub const PLUG: &str = "icons/duotone/plug-duotone.svg";
pub const PLUG_CHARGING: &str = "icons/duotone/plug-charging-duotone.svg";
pub const ROBOT: &str = "icons/duotone/robot-duotone.svg";
pub const SHIELD_CHECK: &str = "icons/duotone/shield-check-duotone.svg";
pub const SPARKLE: &str = "icons/duotone/sparkle-duotone.svg";
pub const SQUARES_FOUR: &str = "icons/regular/squares-four.svg";
pub const STACK: &str = "icons/duotone/stack-duotone.svg";
pub const STAR_FILL: &str = "icons/regular/star.svg";
pub const TERMINAL_WINDOW: &str = "icons/regular/terminal-window.svg";
pub const TRASH: &str = "icons/regular/trash-simple.svg";
pub const UPLOAD_SIMPLE: &str = "icons/regular/upload-simple.svg";
pub const WIFI_HIGH: &str = "icons/duotone/wifi-high-duotone.svg";
pub const X: &str = "icons/regular/x.svg";

/// Render a duotone glyph at `size`, tinted with a single color (§3.2).
pub fn icon(path: &str, size: Pixels, color: Hsla) -> Svg {
    svg()
        .path(path)
        .flex_shrink_0()
        .size(size)
        .text_color(color)
}

/// The Android18 brand mark: the canonical `icons/AppIcon.png` — the same
/// export the runtime Dock tile and every bundle `.icns` are built from —
/// decoded once per process and cached. Powers the About modal's identity
/// row; `None` only if the embedded asset fails to decode (it is pinned by
/// `platform.rs`'s square-PNG tests).
pub fn app_icon() -> Option<Arc<RenderImage>> {
    static CACHE: OnceLock<Option<Arc<RenderImage>>> = OnceLock::new();
    CACHE
        .get_or_init(|| crate::thumb::decode(crate::platform::APP_ICON_PNG).map(Arc::new))
        .clone()
}

/// §3.6 file-category → (glyph, tint) mapping.
pub fn category_glyph(category: FileCategory) -> (&'static str, Hsla) {
    match category {
        FileCategory::Folder => (FOLDER, theme::SLATE_500),
        FileCategory::Image | FileCategory::BinaryImage => (IMAGE, theme::AMBER_500),
        FileCategory::Video => (FILM_STRIP, theme::PURPLE_500),
        FileCategory::Audio => (MUSIC_NOTES, theme::SKY_500),
        FileCategory::Document | FileCategory::MarkdownNote | FileCategory::Spreadsheet => {
            (FILE_TEXT, theme::EMERALD_500)
        }
        FileCategory::CodeConfig => (FILE_CODE, theme::EMERALD_500),
        FileCategory::Archive => (FILE_ZIP, theme::ROSE_500),
        FileCategory::AndroidPackage => (ANDROID_LOGO, theme::ROSE_500),
        FileCategory::File => (FILE, theme::SLATE_400),
    }
}

/// Glyph + tint for an entry (folders always use the folder glyph, §3.3).
pub fn entry_glyph(entry: &Entry) -> (&'static str, Hsla) {
    category_glyph(FileCategory::of(entry))
}
