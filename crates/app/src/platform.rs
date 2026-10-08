//! Platform integration.
//!
//! The Dock / app icon: GPUI's `WindowOptions.icon` is X11-only (see
//! `gpui-pre`'s `platform.rs`), so on macOS the AppKit application icon is
//! installed at runtime from the canonical flattened export
//! `icons/AppIcon.png` — the single icon source, also compiled into every
//! bundle `.icns` by `scripts/make-icns.sh`, so the dev Dock tile and the
//! packaged app can never drift apart. `scripts/dev.sh` additionally
//! launches the binary from a generated dev `.app` wrapper because only the
//! Dock tile honors a runtime `setApplicationIconImage`: surfaces that
//! resolve the icon from the bundle (Stage Manager, Mission Control
//! grouping, ⌘-Tab) need a real `CFBundleIconFile`.

/// The canonical flattened app-icon export — one embedded copy shared by
/// the macOS Dock-tile install below and the About modal's identity mark
/// (`icon::app_icon`); `scripts/make-icns.sh` compiles the same file into
/// every bundle `.icns`.
pub(crate) const APP_ICON_PNG: &[u8] = include_bytes!("icons/AppIcon.png");

/// Sets the application icon.
///
/// Failures are logged and degrade to the system default icon — a missing
/// icon must never block startup.
pub fn set_app_icon() {
    #[cfg(target_os = "macos")]
    if let Err(err) = install_appkit_icon() {
        eprintln!("android18: failed to set the app icon: {err}");
    }
}

/// `NSApplication.setApplicationIconImage` with an `NSImage` decoded from
/// the bundled app icon. AppKit parses the PNG itself, so no image crate is
/// involved.
#[cfg(target_os = "macos")]
fn install_appkit_icon() -> Result<(), Box<dyn std::error::Error>> {
    use objc2::AnyThread as _;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    // AppKit's shared application object is main-thread-only; GPUI runs the
    // startup closure on the main thread, so the marker is always available.
    let mtm = MainThreadMarker::new().ok_or("app icon set off the main thread")?;
    let png = NSData::with_bytes(APP_ICON_PNG);

    let icon = NSImage::initWithData(NSImage::alloc(), &png)
        .ok_or("icons/AppIcon.png is not decodable image data")?;

    // SAFETY: `setApplicationIconImage:` is main-thread-only and we hold the
    // main-thread marker; `icon` is a fully initialized NSImage.
    unsafe {
        NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&icon));
    }
    Ok(())
}

/// Sets (or clears, with `None`) the Dock tile badge. No-op off macOS.
pub fn set_dock_badge(label: Option<&str>) {
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSApplication;
        use objc2_foundation::NSString;

        // Called from render, which GPUI runs on the main thread.
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let label = label.map(NSString::from_str);
        let tile = NSApplication::sharedApplication(mtm).dockTile();
        tile.setBadgeLabel(label.as_deref());
    }
    #[cfg(not(target_os = "macos"))]
    let _ = label;
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::APP_ICON_PNG;

    /// Decodes a PNG's IHDR into `(width, height)`; `None` for anything that
    /// is not a PNG with a leading IHDR chunk. Std-only on purpose — AppKit
    /// proves real decodability at runtime.
    fn png_dimensions(png: &[u8]) -> Option<(u32, u32)> {
        const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        if png.len() < 24 || png[..8] != SIGNATURE || png[12..16] != *b"IHDR" {
            return None;
        }
        let width = u32::from_be_bytes(png[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(png[20..24].try_into().ok()?);
        Some((width, height))
    }

    /// 1024px square — what `make-icns.sh` needs for the 512pt@2x icns
    /// member and the largest size the Dock ever requests.
    #[test]
    fn app_icon_is_a_1024_square_png() {
        let (width, height) =
            png_dimensions(APP_ICON_PNG).expect("icons/AppIcon.png must be a PNG");
        assert_eq!((width, height), (1024, 1024));
    }

    /// Keeps the optimized export honest — an unoptimized re-export would
    /// bloat every binary by the difference.
    #[test]
    fn app_icon_stays_lean() {
        assert!(
            APP_ICON_PNG.len() <= 180 * 1024,
            "icons/AppIcon.png grew to {} bytes — recompress it before shipping",
            APP_ICON_PNG.len()
        );
    }
}
