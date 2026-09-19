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
}

pub fn settings(cx: &App) -> &Settings {
    &cx.global::<AppSettings>().0
}

/// Change settings and save them.
pub fn update_settings(cx: &mut App, f: impl FnOnce(&mut Settings)) {
    let s = &mut cx.global_mut::<AppSettings>().0;
    f(s);
    if let Err(e) = s.save() {
        tracing::warn!(error = %e, "could not save settings");
    }
    cx.refresh_windows();
}

pub fn cli(cx: &App) -> CliStatus {
    cx.global::<Capabilities>().cli.clone()
}

/// Look for the coding CLI in the background.
pub fn detect_cli(cx: &mut App) {
    cx.global_mut::<Capabilities>().cli = CliStatus::Checking;
    let explicit = settings(cx).cli_path.clone();
    let binary = emulsion_assistant::provider::default_provider().binary;
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
