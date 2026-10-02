//! Interface language: the saved choice, else the system's when we ship it,
//! else English. Strings live in `locales/<code>.json`; GPUI components share
//! the same rust-i18n locale, so their built-in text switches with ours.

use crate::app_state;
use gpui_kit::App;

/// Shipped languages as (locale code, name in that language).
pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("es", "Español"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("pt-BR", "Português (Brasil)"),
    ("pl", "Polski"),
    ("ja", "日本語"),
    ("zh-CN", "简体中文"),
];

/// The shipped language for a BCP 47 or POSIX tag ("de-AT", "pt_PT.UTF-8").
/// Region only matters for Chinese, where Traditional script is not shipped.
pub fn supported(tag: &str) -> Option<&'static str> {
    let tag = tag.split(['.', '@']).next()?.replace('_', "-");
    let lower = tag.to_ascii_lowercase();
    if let Some(&(code, _)) = LANGUAGES
        .iter()
        .find(|(code, _)| code.eq_ignore_ascii_case(&lower))
    {
        return Some(code);
    }
    let mut parts = lower.split('-');
    let primary = parts.next()?;
    if primary == "zh" {
        let traditional = parts.any(|p| matches!(p, "hant" | "tw" | "hk" | "mo"));
        return (!traditional).then_some("zh-CN");
    }
    LANGUAGES
        .iter()
        .find(|(code, _)| code.split('-').next() == Some(primary))
        .map(|(code, _)| *code)
}

/// The language to show: a saved choice wins, then the system's, then English.
pub fn resolve(saved: &str, system: Option<&str>) -> &'static str {
    supported(saved)
        .or_else(|| system.and_then(supported))
        .unwrap_or("en")
}

/// Use the saved or system language for every string drawn from now on.
pub fn apply_saved(cx: &App) {
    let system = sys_locale::get_locale();
    rust_i18n::set_locale(resolve(
        &app_state::settings(cx).language,
        system.as_deref(),
    ));
}

/// Save a language choice ("" follows the system); windows redraw in it.
pub fn set_language(code: &str, cx: &mut App) {
    app_state::update_settings(cx, |s| s.language = code.to_string());
    apply_saved(cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn system_tags_map_to_shipped_languages() {
        assert_eq!(supported("de-AT"), Some("de"));
        assert_eq!(supported("pt_PT.UTF-8"), Some("pt-BR"));
        assert_eq!(supported("fr_CA@euro"), Some("fr"));
        assert_eq!(supported("zh-Hans-CN"), Some("zh-CN"));
        assert_eq!(supported("zh_TW"), None);
        assert_eq!(supported("pl_PL.UTF-8"), Some("pl"));
        assert_eq!(supported("C"), None);
        assert_eq!(supported(""), None);
        assert_eq!(resolve("", Some("ja-JP")), "ja");
        assert_eq!(resolve("es", Some("ja-JP")), "es");
        assert_eq!(resolve("", Some("ko-KR")), "en");
        assert_eq!(resolve("", None), "en");
    }

    fn locale(code: &str) -> BTreeMap<String, String> {
        let path = format!("{}/locales/{code}.json", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    fn placeholders(text: &str) -> Vec<&str> {
        let mut out: Vec<&str> = text
            .split("%{")
            .skip(1)
            .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
            .collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn every_language_translates_every_key_with_the_same_placeholders() {
        let english = locale("en");
        for &(code, _) in &LANGUAGES[1..] {
            let other = locale(code);
            for (key, text) in &english {
                let translated = other
                    .get(key)
                    .unwrap_or_else(|| panic!("{code} is missing {key}"));
                assert!(!translated.trim().is_empty(), "{code} {key} is empty");
                assert_eq!(
                    placeholders(text),
                    placeholders(translated),
                    "{code} {key} placeholders"
                );
            }
            for key in other.keys() {
                assert!(english.contains_key(key), "{code} has unused key {key}");
            }
        }
    }

    #[test]
    fn missing_translations_fall_back_to_english() {
        assert_eq!(t!("menu.file", locale = "en"), "File");
        assert_eq!(t!("menu.file", locale = "xx"), "File");
    }
}
