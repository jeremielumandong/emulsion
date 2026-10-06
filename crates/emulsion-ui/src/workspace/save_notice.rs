//! Completed native-save disclosure. Call only after the saved snapshot was
//! published successfully, never for recovery writes or an in-progress save.
use std::path::Path;

pub(crate) fn project_notice(version: u32) -> Option<String> {
    (version == 2).then(|| t!("shell.saved_project_format2").into_owned())
}

pub(crate) fn completed_status(
    path: &Path,
    project_version: Option<u32>,
    cloud_result: &Result<bool, String>,
) -> (String, bool) {
    let saved = match cloud_result {
        Ok(true) => t!("shell.saved_cloud_queued", path = path.display()),
        Ok(false) => t!("shell.saved", path = path.display()),
        Err(error) => t!("shell.saved_cloud_failed", error = error),
    };
    // The one-line status strip truncates long paths but its tooltip retains
    // the whole message. Put compatibility first so a path cannot hide it.
    let message = match project_version.and_then(project_notice) {
        Some(notice) => format!("{notice} {saved}"),
        None => saved.into_owned(),
    };
    (message, cloud_result.is_err())
}

#[cfg(test)]
#[path = "save_notice_tests.rs"]
mod tests;
