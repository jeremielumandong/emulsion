//! App-wide state: settings and detected capabilities.

use emulsion_io::settings::Settings;
use gpui_kit::*;
use std::path::PathBuf;

pub struct AppSettings(pub Settings);
impl Global for AppSettings {}

#[derive(Clone, Debug, PartialEq)]
pub enum CliStatus {
    Checking,
    Found { path: PathBuf, version: String },
    Missing,
}

pub struct Capabilities {
    pub cli: CliStatus,
}
impl Global for Capabilities {}

pub fn install(cx: &mut App) {
    cx.set_global(AppSettings(Settings::load()));
    cx.set_global(Capabilities {
        cli: CliStatus::Checking,
    });
    detect_cli(cx);
    let sessions = emulsion_io::recent::data_dir().join("sessions");
    cx.background_executor()
        .spawn(async move {
            if let Err(error) = emulsion_assistant::storage::cleanup_stale(&sessions) {
                tracing::warn!(%error, "could not clean abandoned assistant workspaces");
            }
        })
        .detach();
}

pub fn settings(cx: &App) -> &Settings {
    &cx.global::<AppSettings>().0
}

/// Apply settings immediately and persist them in order off the UI thread.
pub fn update_settings(cx: &mut App, f: impl FnOnce(&mut Settings)) {
    f(&mut cx.global_mut::<AppSettings>().0);
    crate::settings_writer::save(settings(cx).clone(), cx).detach();
    cx.refresh_windows();
}

/// Effective runtime choice, separate from the saved preference so a launch
/// override never locks the live Settings switch or rewrites preferences.
struct LayoutReuse(bool);
impl Global for LayoutReuse {}

pub(crate) fn layout_reuse_launch_override() -> Option<bool> {
    parse_layout_reuse_override(std::env::var_os("EMULSION_RETAINED_LAYOUT").as_deref())
}

fn parse_layout_reuse_override(value: Option<&std::ffi::OsStr>) -> Option<bool> {
    match value.and_then(std::ffi::OsStr::to_str) {
        Some("1") => Some(true),
        Some("0") => Some(false),
        _ => None,
    }
}

/// Seed once per app; later windows inherit any choice made in Settings.
pub(crate) fn initialize_layout_reuse(cx: &mut App) -> bool {
    if !cx.has_global::<LayoutReuse>() {
        let enabled =
            layout_reuse_launch_override().unwrap_or(settings(cx).experimental_layout_reuse);
        cx.set_global(LayoutReuse(enabled));
    }
    layout_reuse_enabled(cx)
}

pub(crate) fn layout_reuse_enabled(cx: &App) -> bool {
    cx.try_global::<LayoutReuse>()
        .map_or(settings(cx).experimental_layout_reuse, |state| state.0)
}

/// Called by the Settings control between frames. Refreshing every window also
/// discards cached view paint/layout ranges before rendering with the new mode.
pub(crate) fn set_layout_reuse_enabled(enabled: bool, window: &mut Window, cx: &mut App) {
    cx.set_global(LayoutReuse(enabled));
    update_settings(cx, |settings| settings.experimental_layout_reuse = enabled);
    window.set_layout_reuse_enabled(enabled);
    for other in cx.windows() {
        // The active window is already borrowed by its input callback.
        if other.window_id() != window.window_handle().window_id() {
            let _ = cx.update_window(other, |_, window, _| {
                window.set_layout_reuse_enabled(enabled);
            });
        }
    }
}

pub fn cli(cx: &App) -> CliStatus {
    cx.global::<Capabilities>().cli.clone()
}

/// Look for the coding CLI in the background.
pub fn detect_cli(cx: &mut App) {
    cx.global_mut::<Capabilities>().cli = CliStatus::Checking;
    let explicit = settings(cx).cli_path.clone();
    let binary = emulsion_assistant::provider::by_id(&settings(cx).provider).binary;
    cx.spawn(async move |cx| {
        let found = cx
            .background_spawn(async move {
                let path = emulsion_assistant::provider::find(binary, explicit.as_deref())?;
                let version = emulsion_assistant::provider::version(&path)
                    .unwrap_or_else(|| "unknown version".into());
                Some((path, version))
            })
            .await;
        cx.update(|cx| {
            cx.global_mut::<Capabilities>().cli = match found {
                Some((path, version)) => CliStatus::Found { path, version },
                None => CliStatus::Missing,
            };
            cx.refresh_windows();
        });
    })
    .detach();
}

#[cfg(test)]
mod layout_reuse_preference_tests {
    use super::parse_layout_reuse_override;
    use std::ffi::OsStr;

    #[test]
    fn launch_override_accepts_explicit_on_and_off_only() {
        assert_eq!(
            parse_layout_reuse_override(Some(OsStr::new("1"))),
            Some(true)
        );
        assert_eq!(
            parse_layout_reuse_override(Some(OsStr::new("0"))),
            Some(false)
        );
        for value in [None, Some(OsStr::new("")), Some(OsStr::new("invalid"))] {
            assert_eq!(parse_layout_reuse_override(value), None);
        }
    }
}
