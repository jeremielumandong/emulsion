//! Emulsion's design tokens: built-in and Omarchy palettes, type, and layout
//! constants. Geometry is square: 1 px hairlines and no rounded corners
//! except avatars and status dots.

use gpui_kit::*;

#[cfg(any(target_os = "linux", test))]
mod omarchy;

#[derive(Clone, Copy, Debug, PartialEq)]
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
    pub accent_fg: Hsla,
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
        accent_fg: c(0xFFFFFF),
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
        accent_fg: c(0xFFFFFF),
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
    /// The same bar in compact chrome.
    pub const TOP_BAR_H_COMPACT: Pixels = px(38.);
    pub const TOOL_RAIL_W: Pixels = px(58.);
    pub const TOOL_BTN_W: Pixels = px(40.);
    pub const TOOL_BTN_H: Pixels = px(38.);
    pub const NODE_PANEL_W: Pixels = px(286.);
    pub const COMPARE_SLIDER_W: Pixels = px(110.);
}

pub struct ActivePalette(pub Palette);
impl Global for ActivePalette {}

/// Install the default palette before applying saved appearance preferences.
pub fn install(cx: &mut App) {
    cx.set_global(ActivePalette(dark()));
}

/// Use the theme saved in settings.
pub fn apply_saved(cx: &mut App) {
    let light = crate::app_state::settings(cx).light_mode;
    let fallback = if light { self::light() } else { dark() };
    #[cfg(target_os = "linux")]
    let fallback = if following_omarchy(cx) {
        omarchy::read_current().unwrap_or(fallback)
    } else {
        fallback
    };
    cx.set_global(ActivePalette(fallback));
    sync_kit(cx);
}

/// Match gpui-kit's widgets (inputs, menus) to the palette. The design is
/// square, so corner radii stay zero in both modes.
pub fn sync_kit(cx: &mut App) {
    use gpui_kit::component::{Theme, ThemeMode};
    let mode = if palette(cx).dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, None, cx);
    let t = Theme::global_mut(cx);
    t.radius = px(0.);
    t.radius_lg = px(0.);
    if following_omarchy(cx) {
        let p = palette(cx);
        let t = Theme::global_mut(cx);
        map_kit_colors(&mut t.colors, p);
        t.tokens = t.colors.into();
    }
    Theme::sync_base(cx);
}

/// Manual choices always exit follow mode, even when the brightness is unchanged.
pub fn set_dark(dark_on: bool, cx: &mut App) {
    crate::app_state::update_settings(cx, |s| {
        s.follow_omarchy = false;
        s.light_mode = !dark_on;
    });
    cx.set_global(ActivePalette(if dark_on { dark() } else { light() }));
    sync_kit(cx);
    cx.refresh_windows();
}

/// The current Omarchy theme's name (its `theme.name` file), for labels.
#[cfg(target_os = "linux")]
pub fn omarchy_theme_name() -> Option<String> {
    omarchy::current_name()
}

pub fn following_omarchy(cx: &App) -> bool {
    cfg!(target_os = "linux") && crate::app_state::settings(cx).follow_omarchy
}

#[cfg(target_os = "linux")]
pub fn follow_omarchy(cx: &mut App) {
    crate::app_state::update_settings(cx, |s| s.follow_omarchy = true);
    apply_saved(cx);
    cx.refresh_windows();
}

/// Poll the path rather than an inode: Omarchy replaces the entire directory.
/// Only read files while following; failed reads retain the last valid palette.
#[cfg(target_os = "linux")]
pub fn watch_omarchy(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(1))
                .await;
            if !cx.update(|cx| following_omarchy(cx)) {
                continue;
            }
            let next = cx.background_spawn(async { omarchy::read_current() }).await;
            cx.update(|cx| apply_external(next, cx));
        }
    })
    .detach();
}

#[cfg(any(target_os = "linux", test))]
fn apply_external(next: Option<Palette>, cx: &mut App) {
    // Recheck after the background read so a manual choice wins over an in-flight read.
    if following_omarchy(cx)
        && let Some(next) = next
        && next != palette(cx)
    {
        cx.set_global(ActivePalette(next));
        sync_kit(cx);
        cx.refresh_windows();
    }
}

/// Project the application palette into GPUI's component colors. Keep semantic
/// warning/error colors from its light/dark theme.
fn map_kit_colors(t: &mut gpui_kit::component::ThemeColor, p: Palette) {
    t.background = p.paper;
    t.foreground = p.ink;
    t.border = p.line;
    t.input = p.line;
    t.caret = p.ink;
    t.accent = p.accent;
    t.accent_foreground = p.accent_fg;
    t.primary = p.accent;
    t.primary_hover = p.accent;
    t.primary_active = p.accent;
    t.primary_foreground = p.accent_fg;
    t.button_primary = p.accent;
    t.button_primary_hover = p.accent;
    t.button_primary_active = p.accent;
    t.button_primary_foreground = p.accent_fg;
    t.button = p.soft_bg;
    t.button_hover = p.soft_bg;
    t.button_active = p.line;
    t.button_foreground = p.ink;
    t.secondary = p.soft_bg;
    t.secondary_hover = p.soft_bg;
    t.secondary_active = p.line;
    t.secondary_foreground = p.ink;
    t.button_secondary = p.soft_bg;
    t.button_secondary_hover = p.soft_bg;
    t.button_secondary_active = p.line;
    t.button_secondary_foreground = p.ink;
    t.muted = p.soft_bg;
    t.muted_foreground = p.muted;
    t.popover = p.panel;
    t.popover_foreground = p.ink;
    t.list = p.panel;
    t.list_even = p.paper;
    t.list_head = p.soft_bg;
    t.list_hover = p.soft_bg;
    t.list_active = p.soft_bg;
    t.list_active_border = p.accent;
    t.ring = p.accent;
    t.selection = p.accent.opacity(0.25);
    t.link = p.accent;
    t.link_hover = p.accent;
    t.link_active = p.accent;
    t.scrollbar = p.paper;
    t.scrollbar_thumb = p.line;
    t.scrollbar_thumb_hover = p.muted;
    t.drag_border = p.accent;
    t.title_bar = p.chrome;
    t.title_bar_border = p.chrome_line;
    t.window_border = p.line;
    t.sidebar = p.panel;
    t.sidebar_foreground = p.ink;
    t.sidebar_border = p.line;
    t.sidebar_accent = p.accent;
    t.sidebar_accent_foreground = p.accent_fg;
    t.sidebar_primary = p.accent;
    t.sidebar_primary_foreground = p.accent_fg;
    t.slider_bar = p.accent;
    t.slider_thumb = p.accent;
    t.progress_bar = p.accent;
    t.switch = p.line;
    t.switch_thumb = p.ink;
    t.tab = p.paper;
    t.tab_bar = p.paper;
    t.tab_bar_segmented = p.paper;
    t.tab_foreground = p.muted;
    t.tab_active = p.panel;
    t.tab_active_foreground = p.ink;
    t.accordion = p.panel;
    t.group_box = p.panel;
    t.group_box_foreground = p.ink;
    t.status_bar = p.chrome;
    t.status_bar_border = p.chrome_line;
}

pub fn palette(cx: &App) -> Palette {
    cx.global::<ActivePalette>().0
}

pub fn toggle(cx: &mut App) {
    let dark_on = !palette(cx).dark;
    set_dark(dark_on, cx);
}

#[cfg(test)]
mod tests {
    use super::{ActivePalette, apply_external, c, dark, install, light, palette, sync_kit};
    use gpui_kit::TestAppContext;

    #[gpui_kit::test]
    fn external_updates_sync_widgets_and_ignore_failed_or_disabled_reads(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let settings = emulsion_io::settings::Settings {
                follow_omarchy: true,
                ..Default::default()
            };
            cx.set_global(crate::app_state::AppSettings(settings));
            install(cx);
            let mut external = dark();
            external.paper = c(0x1e1e2e);
            external.accent = c(0x89b4fa);
            external.accent_fg = c(0);
            apply_external(Some(external), cx);
            if cfg!(target_os = "linux") {
                assert_eq!(palette(cx), external);
                let kit = gpui_kit::component::Theme::global(cx);
                assert_eq!(kit.background, external.paper);
                assert_eq!(kit.primary_foreground, external.accent_fg);
                assert_eq!(kit.tokens.popover.color, external.panel);
                apply_external(None, cx);
                assert_eq!(
                    palette(cx),
                    external,
                    "a failed read must not flash the fallback"
                );
            } else {
                assert_eq!(palette(cx), dark(), "non-Linux ignores Omarchy settings");
            }
            cx.global_mut::<crate::app_state::AppSettings>()
                .0
                .follow_omarchy = false;
            cx.set_global(ActivePalette(light()));
            sync_kit(cx);
            apply_external(Some(external), cx);
            assert_eq!(
                palette(cx),
                light(),
                "a late read must not override a manual choice"
            );
        });
    }
}
