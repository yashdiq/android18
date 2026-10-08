//! Design tokens — the single home for every color, radius and font constant.
//!
//! Values mirror `docs/DESIGN.md` §2 (colors), §4 (typography) and §5
//! (spacing/radii). Components must reference these constants instead of
//! hardcoding hex values, so the palette stays auditable in one place.
//!
//! The full §2 ramp stays defined even where no surface uses a step yet.
#![allow(dead_code)]

use android18_core::domain::ColorTag;
use gpui_kit::{Hsla, Pixels, px};

/// Convert a `0xRRGGBB` hex literal into GPUI's HSL color space at full alpha.
pub const fn rgb(hex: u32) -> Hsla {
    let r = ((hex >> 16) & 0xff) as f32 / 255.0;
    let g = ((hex >> 8) & 0xff) as f32 / 255.0;
    let b = (hex & 0xff) as f32 / 255.0;
    let max = if r > g {
        if r > b { r } else { b }
    } else if g > b {
        g
    } else {
        b
    };
    let min = if r < g {
        if r < b { r } else { b }
    } else if g < b {
        g
    } else {
        b
    };
    let l = (max + min) / 2.0;
    if max == min {
        return Hsla {
            h: 0.0,
            s: 0.0,
            l,
            a: 1.0,
        };
    }
    let d = max - min;
    let s = d / (if l > 0.5 { 2.0 - max - min } else { max + min });
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;
    Hsla {
        h: h / 360.0,
        s,
        l,
        a: 1.0,
    }
}

// ---------------------------------------------------------------------------
// §4 Typography — families registered in `assets.rs`.
// ---------------------------------------------------------------------------
/// UI text family (§4): Plus Jakarta Sans 400/500/600/700.
pub const FONT_SANS: &str = "Plus Jakarta Sans";
/// Terminal, paths and metrics (§4): JetBrains Mono 400/500/600.
pub const FONT_MONO: &str = "JetBrains Mono";

// ---------------------------------------------------------------------------
// §2.1 Ink & surfaces (Tailwind slate ramp).
// ---------------------------------------------------------------------------
pub const WHITE: Hsla = rgb(0xffffff);
/// Ink color (DESIGN.md deviation note): true black instead of slate-900 for
/// text ink, selected states, progress fills and dark pills. Primary button
/// fills follow the prototype's slate-900 (see [`SLATE_900`]).
pub const BLACK: Hsla = rgb(0x000000);
pub const SLATE_50: Hsla = rgb(0xf8fafc);
pub const SLATE_100: Hsla = rgb(0xf1f5f9);
pub const SLATE_200: Hsla = rgb(0xe2e8f0);
pub const SLATE_300: Hsla = rgb(0xcbd5e1);
pub const SLATE_400: Hsla = rgb(0x94a3b8);
pub const SLATE_500: Hsla = rgb(0x64748b);
pub const SLATE_600: Hsla = rgb(0x475569);
pub const SLATE_700: Hsla = rgb(0x334155);
pub const SLATE_800: Hsla = rgb(0x1e293b);
/// Prototype primary-button fill (`bg-slate-900`).
pub const SLATE_900: Hsla = rgb(0x0f172a);
pub const SLATE_950: Hsla = rgb(0x020617);

// ---------------------------------------------------------------------------
// §2.2 Accent families. Blue survives only as the folder ColorTag swatch —
// every interactive accent is slate (§2.1).
// ---------------------------------------------------------------------------
pub const BLUE_600: Hsla = rgb(0x2563eb);

/// Drag-and-drop upload target: tinted fill + ring (purple is AI-only).
pub const DROP_BG: Hsla = rgb(0xdbeafe);
pub const DROP_RING: Hsla = BLUE_600;

pub const EMERALD_50: Hsla = rgb(0xecfdf5);
pub const EMERALD_100: Hsla = rgb(0xd1fae5);
pub const EMERALD_500: Hsla = rgb(0x10b981);
pub const EMERALD_600: Hsla = rgb(0x059669);
pub const EMERALD_700: Hsla = rgb(0x047857);

pub const AMBER_50: Hsla = rgb(0xfffbeb);
pub const AMBER_100: Hsla = rgb(0xfef3c7);
pub const AMBER_500: Hsla = rgb(0xf59e0b);
pub const AMBER_600: Hsla = rgb(0xd97706);

pub const ROSE_50: Hsla = rgb(0xfff1f2);
pub const ROSE_500: Hsla = rgb(0xf43f5e);
pub const ROSE_600: Hsla = rgb(0xe11d48);

pub const SKY_500: Hsla = rgb(0x0ea5e9);

// ---------------------------------------------------------------------------
// §2.3 Purple — AI surfaces only (never a generic accent).
// ---------------------------------------------------------------------------
pub const PURPLE_200: Hsla = rgb(0xe9d5ff);
pub const PURPLE_300: Hsla = rgb(0xd8b4fe);
pub const PURPLE_500: Hsla = rgb(0xa855f7);
pub const PURPLE_600: Hsla = rgb(0x9333ea);
pub const PURPLE_800: Hsla = rgb(0x6b21a8);
pub const PURPLE_950: Hsla = rgb(0x2e1065);

// ---------------------------------------------------------------------------
// §5 Radii.
// ---------------------------------------------------------------------------
pub const RADIUS_MD: Pixels = px(6.);
pub const RADIUS_LG: Pixels = px(8.);
pub const RADIUS_XL: Pixels = px(12.);
pub const RADIUS_2XL: Pixels = px(16.);

/// §2.4 color-tag dot palette.
pub fn tag_color(tag: ColorTag) -> Hsla {
    match tag {
        ColorTag::Blue => BLUE_600,
        ColorTag::Emerald => EMERALD_600,
        ColorTag::Amber => AMBER_600,
        ColorTag::Purple => PURPLE_600,
        ColorTag::Rose => ROSE_600,
        ColorTag::Slate => SLATE_500,
    }
}
