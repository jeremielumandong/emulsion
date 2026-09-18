//! Emulsion's design tokens.
//!
//! Light and dark palettes, plus the type sizes the mock uses. Kept as plain
//! values so the same tokens can drive gpui-component's `Theme` and our own
//! elements.

use gpui_kit::*;

/// One palette.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub paper: Hsla,
    pub panel: Hsla,
    pub ink: Hsla,
    pub muted: Hsla,
    pub line: Hsla,
    pub stage: Hsla,
    pub chrome: Hsla,
    pub chrome_fg: Hsla,
    pub chrome_line: Hsla,
    pub nav_fg: Hsla,
    pub soft_bg: Hsla,
    pub accent: Hsla,
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub const ACCENT: u32 = 0xD93A1E;

pub fn light() -> Palette {
    Palette {
        paper: c(0xEFEEEA),
        panel: c(0xFFFFFF),
        ink: c(0x0A0A0B),
        muted: c(0x6E6D68),
        line: c(0xD7D5CE),
        stage: c(0xE4E2DC),
        chrome: c(0x0A0A0B),
        chrome_fg: c(0xEFEEEA),
        chrome_line: c(0x232326),
        nav_fg: c(0x9B9A95),
        soft_bg: c(0xFFFFFF),
        accent: c(ACCENT),
    }
}

pub fn dark() -> Palette {
    Palette {
        paper: c(0x0C0C0D),
        panel: c(0x151517),
        ink: c(0xEDECE8),
        muted: c(0x8B8A85),
        line: c(0x26262A),
        stage: c(0x131315),
        chrome: c(0x000000),
        chrome_fg: c(0xEDECE8),
        chrome_line: c(0x1E1E21),
        nav_fg: c(0x7C7B77),
        soft_bg: c(0x1C1C1F),
        accent: c(ACCENT),
    }
}

/// Type system: Instrument Sans for UI, JetBrains Mono for metadata.
/// Fonts are not bundled yet (Phase 0); GPUI falls back to the system font.
pub const UI_FONT: &str = "Instrument Sans";
pub const MONO_FONT: &str = "JetBrains Mono";

/// Layout constants. Kept as constants so later web and tablet layouts can
/// derive their breakpoints from them.
pub mod dim {
    use gpui_kit::{Pixels, px};
    pub const TOP_BAR_H: Pixels = px(54.);
    pub const TOOL_RAIL_W: Pixels = px(58.);
    pub const TOOL_BTN_W: Pixels = px(40.);
    pub const TOOL_BTN_H: Pixels = px(38.);
    pub const NODE_PANEL_W: Pixels = px(286.);
    pub const COMPARE_SLIDER_W: Pixels = px(110.);
}

/// Global palette handle. Phase 0: light only; dark comes with the theme
/// switch in the top bar.
pub struct ActivePalette(pub Palette);
impl Global for ActivePalette {}

pub fn install(cx: &mut App) {
    cx.set_global(ActivePalette(light()));
}

pub fn palette(cx: &App) -> Palette {
    cx.global::<ActivePalette>().0
}
