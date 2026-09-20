//! Omarchy palettes are ordinary TOML; only discovery touches the host OS.

use super::{Palette, c, dark, light};
use gpui_kit::Hsla;

fn luminance(hex: u32) -> f64 {
    let channel = |shift: u32| {
        let value = f64::from((hex >> shift) & 255u32) / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

/// Choose the higher WCAG contrast ratio, including for bright theme accents.
pub(super) fn contrast_foreground(hex: u32) -> Hsla {
    let luminance = luminance(hex);
    c(if (luminance + 0.05) / 0.05 >= 1.05 / (luminance + 0.05) {
        0x000000
    } else {
        0xffffff
    })
}

fn blend(background: u32, foreground: u32, percent: u32) -> u32 {
    let channel = |shift: u32| {
        (((background >> shift) & 255u32) * (100 - percent)
            + ((foreground >> shift) & 255u32) * percent)
            / 100
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

fn parse(source: &str) -> Option<Palette> {
    let table = source.parse::<toml::Table>().ok()?;
    // Validate every supplied color, even optional ones, before using the palette.
    let mut colors = std::collections::HashMap::new();
    for (name, value) in &table {
        if name == "mode" {
            continue;
        }
        let recognized = matches!(
            name.as_str(),
            "accent"
                | "selection"
                | "muted"
                | "background"
                | "dark_background"
                | "darker_background"
                | "lighter_background"
                | "foreground"
                | "dark_foreground"
                | "light_foreground"
                | "bright_foreground"
                | "red"
                | "yellow"
                | "orange"
                | "green"
                | "cyan"
                | "blue"
                | "magenta"
                | "brown"
                | "bright_red"
                | "bright_yellow"
                | "bright_green"
                | "bright_cyan"
                | "bright_blue"
                | "bright_magenta"
        ) || name
            .strip_prefix("color")
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| n < 16);
        if !recognized {
            continue;
        }
        let raw = value.as_str()?.strip_prefix('#')?;
        if raw.len() != 6 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        colors.insert(name.as_str(), u32::from_str_radix(raw, 16).ok()?);
    }
    let get = |name: &str| colors.get(name).copied();
    let background = get("background").or_else(|| get("color0"))?;
    let foreground = get("foreground").or_else(|| get("color7"))?;
    let accent = get("accent").or_else(|| get("color4"))?;
    let is_dark = match table.get("mode") {
        Some(value) => match value.as_str()? {
            "dark" => true,
            "light" => false,
            _ => return None,
        },
        None => luminance(background) < 0.179,
    };
    let mut palette = if is_dark { dark() } else { light() };
    let panel = get("dark_background").unwrap_or_else(|| blend(background, foreground, 4));
    let line = get("muted").unwrap_or_else(|| blend(background, foreground, 20));
    let muted = get("dark_foreground").unwrap_or_else(|| blend(background, foreground, 65));
    palette.paper = c(background);
    palette.panel = c(panel);
    palette.ink = c(foreground);
    palette.muted = c(muted);
    palette.line = c(line);
    palette.chrome = c(get("darker_background").unwrap_or(panel));
    palette.chrome_fg = c(foreground);
    palette.chrome_line = c(line);
    palette.nav_fg = c(muted);
    palette.soft_bg = c(get("lighter_background")
        .or_else(|| get("selection"))
        .unwrap_or_else(|| blend(background, foreground, 10)));
    palette.accent = c(accent);
    palette.accent_fg = contrast_foreground(accent);
    Some(palette)
}

#[cfg(target_os = "linux")]
fn read_paths(paths: impl IntoIterator<Item = std::path::PathBuf>) -> Option<Palette> {
    use std::io::Read;
    for path in paths {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        // A colors file is tiny. Bound reads in case the path points elsewhere.
        let mut source = String::new();
        file.take(64 * 1024 + 1).read_to_string(&mut source).ok()?;
        if source.len() > 64 * 1024 {
            return None;
        }
        return parse(&source);
    }
    None
}

/// Roots that may hold `omarchy/current/…`: XDG state, then config.
#[cfg(target_os = "linux")]
fn omarchy_roots() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let xdg_path = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let state =
        xdg_path("XDG_STATE_HOME").or_else(|| home.as_ref().map(|home| home.join(".local/state")));
    let config =
        xdg_path("XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|home| home.join(".config")));
    state.into_iter().chain(config).collect()
}

/// The name Omarchy records for the current theme, e.g. "tokyo-night".
#[cfg(target_os = "linux")]
pub(super) fn current_name() -> Option<String> {
    omarchy_roots().into_iter().find_map(|root| {
        let text = std::fs::read_to_string(root.join("omarchy/current/theme.name")).ok()?;
        let name = text.trim();
        (!name.is_empty() && name.len() <= 64).then(|| name.to_string())
    })
}

#[cfg(target_os = "linux")]
pub(super) fn read_current() -> Option<Palette> {
    use std::path::PathBuf;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let xdg_path = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let state =
        xdg_path("XDG_STATE_HOME").or_else(|| home.as_ref().map(|home| home.join(".local/state")));
    let config =
        xdg_path("XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|home| home.join(".config")));
    read_paths(
        state
            .into_iter()
            .chain(config)
            .map(|root| root.join("omarchy/current/theme/colors.toml")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARK: &str = "background = '#101020'\nforeground = '#eeeeff'\naccent = '#ffcc00'";

    #[test]
    fn maps_modern_colors_and_keeps_document_stage_neutral() {
        let palette = parse(&format!("{DARK}\nmode = 'dark'\ndark_background = '#202030'\nmuted = '#444455'\ndark_foreground = '#aaaabb'" )).unwrap();
        assert!(palette.dark);
        assert_eq!(palette.paper, c(0x101020));
        assert_eq!(palette.panel, c(0x202030));
        assert_eq!(palette.line, c(0x444455));
        assert_eq!(palette.muted, c(0xaaaabb));
        assert_eq!(palette.accent_fg, c(0));
        assert_eq!(palette.stage, dark().stage);
        assert_eq!(palette.checker, dark().checker);
    }

    #[test]
    fn infers_mode_and_respects_explicit_light_mode() {
        assert!(parse(DARK).unwrap().dark);
        let palette =
            parse("background = '#ffffff'\nforeground = '#111111'\naccent = '#001177'").unwrap();
        assert!(!palette.dark);
        assert_eq!(palette.stage, light().stage);
        assert_eq!(palette.checker, light().checker);
        assert!(!parse(&format!("{DARK}\nmode = 'light'")).unwrap().dark);
    }

    #[test]
    fn supports_legacy_terminal_palette() {
        let palette = parse("color0 = '#101020'\ncolor7 = '#eeeeff'\ncolor4 = '#ffcc00'").unwrap();
        assert_eq!(palette, parse(DARK).unwrap());
    }

    #[test]
    fn rejects_missing_and_malformed_colors() {
        for source in ["", "[broken", "background = '#ffffff'", "background = 123"] {
            assert!(parse(source).is_none(), "{source}");
        }
        for extra in [
            "muted = '#abc'",
            "selection = '123456'",
            "color15 = '#zzzzzz'",
            "dark_foreground = 42",
            "mode = 'sepia'",
        ] {
            assert!(parse(&format!("{DARK}\n{extra}")).is_none(), "{extra}");
        }
        assert!(parse(&DARK.replace("#ffcc00", "#ffcc0000")).is_none());
    }

    #[test]
    fn accent_text_has_at_least_four_point_five_contrast() {
        for color in [0, 0xffffff, 0xffcc00, 0x89b4fa, 0xd93a1e, 0x777777] {
            let text = contrast_foreground(color);
            let lum = luminance(color);
            let ratio = if text == c(0) {
                (lum + 0.05) / 0.05
            } else {
                1.05 / (lum + 0.05)
            };
            assert!(ratio >= 4.5);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rereads_replaced_directory_and_falls_back_only_when_absent() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("emulsion-theme-{}-{unique}", std::process::id()));
        let current = root.join("theme");
        let file = current.join("colors.toml");
        let legacy = root.join("legacy.toml");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(&file, DARK).unwrap();
        std::fs::write(&legacy, DARK).unwrap();
        let read = || read_paths([file.clone(), legacy.clone()]);
        assert!(read().unwrap().dark);
        std::fs::rename(&current, root.join("old-theme")).unwrap();
        std::fs::create_dir(&current).unwrap();
        std::fs::write(&file, format!("{DARK}\nmode = 'light'")).unwrap();
        assert!(!read().unwrap().dark);
        std::fs::write(&file, "broken").unwrap();
        assert!(read().is_none());
        std::fs::remove_file(&file).unwrap();
        assert!(read().unwrap().dark);
        std::fs::remove_file(&legacy).unwrap();
        assert!(read().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
