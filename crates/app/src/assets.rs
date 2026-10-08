//! Asset pipeline: vendored Phosphor icons + the app-icon asset (rust-embed
//! over `src/icons/`) composed ahead of the gpui-kit assets, plus registration
//! of the bundled Plus Jakarta Sans / JetBrains Mono families (§4).

use std::borrow::Cow;

use gpui_kit::assets::Assets as KitAssets;
use gpui_kit::{App, AssetSource, Result, SharedString};
use rust_embed::RustEmbed;

/// The vendored Phosphor duotone set — UI glyphs only. The app icon already
/// ships twice (`platform.rs` `include_bytes!` + the bundle `.icns` built by
/// `scripts/make-icns.sh`), and Finder droppings must never reach the binary.
///
/// In debug builds rust-embed reads from the filesystem, so icon tweaks show
/// up on the next app relaunch without a rebuild (hot-reload friendly).
#[derive(RustEmbed)]
#[folder = "src/icons/"]
#[exclude = "AppIcon.png"]
#[exclude = "AppIcon.icon/**"]
#[exclude = "**/.DS_Store"]
struct Icons;

/// App asset source: app icons win, kit component assets are the fallback.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(local) = path.strip_prefix("icons/")
            && let Some(file) = Icons::get(local)
        {
            return Ok(Some(file.data));
        }
        KitAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = KitAssets.list(path)?;
        paths.extend(Icons::iter().map(|file| format!("icons/{file}").into()));
        Ok(paths)
    }
}

const PJS_REGULAR: &[u8] = include_bytes!("fonts/PlusJakartaSans-Regular.ttf");
const PJS_MEDIUM: &[u8] = include_bytes!("fonts/PlusJakartaSans-Medium.ttf");
const PJS_SEMIBOLD: &[u8] = include_bytes!("fonts/PlusJakartaSans-SemiBold.ttf");
const PJS_BOLD: &[u8] = include_bytes!("fonts/PlusJakartaSans-Bold.ttf");
const JBM_REGULAR: &[u8] = include_bytes!("fonts/JetBrainsMono-Regular.ttf");
const JBM_MEDIUM: &[u8] = include_bytes!("fonts/JetBrainsMono-Medium.ttf");
const JBM_SEMIBOLD: &[u8] = include_bytes!("fonts/JetBrainsMono-SemiBold.ttf");

/// Registers the bundled families (§4). Called once at startup, before any
/// window opens. Font failures degrade to the system family, so they are
/// reported rather than fatal.
pub fn load_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(PJS_REGULAR),
        Cow::Borrowed(PJS_MEDIUM),
        Cow::Borrowed(PJS_SEMIBOLD),
        Cow::Borrowed(PJS_BOLD),
        Cow::Borrowed(JBM_REGULAR),
        Cow::Borrowed(JBM_MEDIUM),
        Cow::Borrowed(JBM_SEMIBOLD),
    ];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        eprintln!("android18: failed to register bundled fonts: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::Icons;

    /// The embed serves UI glyphs only — the app icon ships via
    /// `platform.rs` and the bundle `.icns`, and Finder droppings must never
    /// reach the binary. Debug builds exercise the dynamic (on-disk) impl,
    /// so this also proves the live folder view is filtered.
    #[test]
    fn only_ui_glyphs_are_embedded() {
        assert!(Icons::get("regular/x.svg").is_some());
        assert!(Icons::get("duotone/android-logo-duotone.svg").is_some());
        assert!(Icons::get("AppIcon.png").is_none());
        assert!(Icons::get("AppIcon.icon/icon.json").is_none());
        assert!(Icons::get("AppIcon.icon/Assets/icon.png").is_none());
        assert!(Icons::get(".DS_Store").is_none());
    }
}
