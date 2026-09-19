//! Emulsion's design tokens: two palettes, the type system, and layout
//! constants. Geometry is square: 1 px hairlines and no rounded corners
//! except avatars and status dots.

use gpui_kit::*;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
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
    /// Transparency checkerboard, sRGB grey levels.
    pub checker: (u8, u8),
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub const ACCENT: u32 = 0xD93A1E;

pub fn light() -> Palette {
    Palette {
        dark: false,
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
        checker: (0xFF, 0xE9),
    }
}

pub fn dark() -> Palette {
    Palette {
        dark: true,
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
        checker: (0x3A, 0x30),
    }
}

/// Instrument Sans for UI and headings, JetBrains Mono for metadata, values,
/// labels and state. Both fall back to system fonts when not installed.
pub const UI_FONT: &str = "Instrument Sans";
pub const MONO_FONT: &str = "JetBrains Mono";

pub mod dim {
    use gpui_kit::{Pixels, px};
    pub const TOP_BAR_H: Pixels = px(54.);
    pub const TOOL_RAIL_W: Pixels = px(58.);
    pub const TOOL_BTN_W: Pixels = px(40.);
    pub const TOOL_BTN_H: Pixels = px(38.);
    pub const NODE_PANEL_W: Pixels = px(286.);
    pub const COMPARE_SLIDER_W: Pixels = px(110.);
}

pub struct ActivePalette(pub Palette);
impl Global for ActivePalette {}

pub fn install(cx: &mut App) {
    cx.set_global(ActivePalette(light()));
}

pub fn palette(cx: &App) -> Palette {
    cx.global::<ActivePalette>().0
}

pub fn toggle(cx: &mut App) {
    let next = if palette(cx).dark { light() } else { dark() };
    cx.set_global(ActivePalette(next));
}
