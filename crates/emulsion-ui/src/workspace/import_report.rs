//! Present evidence produced by the same background import; never re-render
//! or infer a profile from layer labels, decoded pixels, or the chosen filename.
use emulsion_io::psd::{ImportProfileDecision, ReadReport};

pub(crate) fn open_notice(
    history_error: Option<&str>,
    report: Option<ReadReport>,
) -> Option<(String, bool)> {
    if let Some(error) = history_error {
        return Some((
            t!("shell.history_unreadable", error = error).into_owned(),
            true,
        ));
    }
    let report = report?;
    let (mut message, warning) = match report.profile_decision {
        ImportProfileDecision::NotCompared => {
            (t!("shell.psd_profile_not_compared").into_owned(), false)
        }
        ImportProfileDecision::UniquePhotoshopSrgbV1 => {
            (t!("shell.psd_profile_selected").into_owned(), false)
        }
        ImportProfileDecision::LegacyMatch { ambiguous: false } => {
            (t!("shell.psd_profile_legacy_match").into_owned(), false)
        }
        ImportProfileDecision::LegacyMatch { ambiguous: true } => {
            (t!("shell.psd_profile_ambiguous").into_owned(), false)
        }
        ImportProfileDecision::SameCurrentAppearance => {
            (t!("shell.psd_profile_same_current").into_owned(), false)
        }
        ImportProfileDecision::SavedAppearance => {
            (t!("shell.psd_saved_appearance").into_owned(), true)
        }
    };
    if report.background_preserved {
        message.push(' ');
        message.push_str(&t!("shell.psd_background_preserved"));
    }
    Some((message, warning))
}

/// A placed fragment/raster may use a different destination profile and does
/// not inherit the source Background role. Label the report as source evidence.
pub(crate) fn source_notice(
    path: &std::path::Path,
    history_error: Option<&str>,
    report: Option<ReadReport>,
) -> Option<(String, bool)> {
    open_notice(history_error, report).map(|(message, warning)| {
        (
            t!(
                "shell.psd_source_notice",
                path = path.display(),
                message = message
            )
            .into_owned(),
            warning,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_import_decision_has_a_bounded_notice_and_severity() {
        for (decision, key, warning) in [
            (
                ImportProfileDecision::UniquePhotoshopSrgbV1,
                "shell.psd_profile_selected",
                false,
            ),
            (
                ImportProfileDecision::LegacyMatch { ambiguous: false },
                "shell.psd_profile_legacy_match",
                false,
            ),
            (
                ImportProfileDecision::LegacyMatch { ambiguous: true },
                "shell.psd_profile_ambiguous",
                false,
            ),
            (
                ImportProfileDecision::SameCurrentAppearance,
                "shell.psd_profile_same_current",
                false,
            ),
            (
                ImportProfileDecision::SavedAppearance,
                "shell.psd_saved_appearance",
                true,
            ),
        ] {
            let report = ReadReport {
                profile_decision: decision,
                background_preserved: false,
            };
            assert_eq!(
                open_notice(None, Some(report)),
                Some((t!(key).into_owned(), warning))
            );
            let (message, severity) = open_notice(
                None,
                Some(ReadReport {
                    background_preserved: true,
                    ..report
                }),
            )
            .unwrap();
            assert!(message.ends_with(t!("shell.psd_background_preserved").as_ref()));
            assert_eq!(severity, warning);
        }
    }

    #[test]
    fn absent_or_uncompared_reports_do_not_claim_pixel_validation() {
        assert!(open_notice(None, None).is_none());
        let message = t!("shell.psd_profile_not_compared").into_owned();
        assert_eq!(
            open_notice(
                None,
                Some(ReadReport {
                    profile_decision: ImportProfileDecision::NotCompared,
                    background_preserved: false
                })
            ),
            Some((message.clone(), false))
        );
        assert_eq!(
            open_notice(
                None,
                Some(ReadReport {
                    profile_decision: ImportProfileDecision::NotCompared,
                    background_preserved: true
                })
            ),
            Some((
                format!("{message} {}", t!("shell.psd_background_preserved")),
                false
            ))
        );
    }

    #[test]
    fn damaged_history_notice_takes_priority_over_profile_or_appearance_info() {
        for decision in [
            ImportProfileDecision::SavedAppearance,
            ImportProfileDecision::UniquePhotoshopSrgbV1,
        ] {
            assert_eq!(
                open_notice(
                    Some("missing history"),
                    Some(ReadReport {
                        profile_decision: decision,
                        background_preserved: true
                    })
                ),
                Some((
                    t!("shell.history_unreadable", error = "missing history").into_owned(),
                    true
                ))
            );
        }
    }
}

#[cfg(test)]
mod open_path_tests {
    use super::*;
    use gpui_kit::TestAppContext;

    #[gpui_kit::test]
    fn primary_background_open_installs_same_import_notice_and_source_metadata(
        cx: &mut TestAppContext,
    ) {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../emulsion-io/tests/fixtures/psd/blending/knockout-deep-nested-pt.psd");
        let (expected, report) = emulsion_io::open_full_with_report(&path).unwrap();
        let expected_notice = open_notice(None, report).unwrap();
        let (workspace, cx) = crate::tests::open(cx, emulsion_core::Document::new(8, 8));
        cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.open_path(path.clone(), window, cx)
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let workspace = workspace.read(cx);
            assert!(workspace.error.is_none());
            assert!(workspace.busy.is_none());
            let editor = workspace.editor.as_ref().unwrap().read(cx);
            crate::document_contents::assert_document_contents(
                &editor.editor.doc,
                &expected.doc,
                "primary background open",
            );
            assert_eq!(editor.source, Some(path));
            let (message, warning) = editor.status.as_ref().unwrap();
            assert_eq!(message.as_ref(), expected_notice.0.as_str());
            assert_eq!(*warning, expected_notice.1);
        });
    }
}
