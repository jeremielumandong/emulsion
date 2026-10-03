//! Paper worksheets in the print dialog: File → Print Worksheets… → Print…
//! sends the worksheet sheets through the dialog's destinations, live
//! preview and submission. Every destination prints at 100% scale (no fit
//! to page), so the corner marks and the code keep the geometry the code
//! describes. On a paper other than the worksheet layout's the sheets are
//! laid out again for it, with a warning.
use super::*;
use anyhow::Context as _;
use emulsion_core::project::Project;
use emulsion_io::storyboard_export::{
    self as story, Profile,
    worksheet::{self, Panels, Worksheets},
};

pub(super) struct WorksheetState {
    project: Arc<Project>,
    panels: Panels,
    /// The layout chosen in Print Worksheets…, with its paper.
    pub(super) profile: Profile,
    date: String,
    /// One batch for the whole dialog, so the preview and the print carry
    /// the same sheet IDs.
    batch: String,
}

/// Open the print dialog on worksheets of `panels` laid out by `profile`.
pub(crate) fn open_worksheets(
    name: String,
    project: Arc<Project>,
    panels: Panels,
    profile: Profile,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| {
        let mut dialog = PrintDialog::new(name, 0, window, cx);
        dialog.attach_worksheets(project, panels, profile);
        dialog
    });
    view.update(cx, |s, cx| s.refresh(cx));
    show(view, t!("print.worksheets.title").into(), window, cx);
}

impl PrintDialog {
    pub(super) fn attach_worksheets(
        &mut self,
        project: Arc<Project>,
        panels: Panels,
        profile: Profile,
    ) {
        self.settings.layout = Layout::Contact;
        self.scope = "all".into();
        // Worksheets are drawn marks only; there is no artwork to prepare.
        self.sources = Some(Arc::new(Vec::new()));
        self.worksheet = Some(WorksheetState {
            project,
            panels,
            profile,
            date: story::today(),
            batch: worksheet::batch_id(),
        });
    }

    /// The worksheet pages on `settings`' paper and orientation, warning
    /// when they differ from the layout's.
    pub(super) fn worksheet_sheets(&self, settings: &Settings) -> anyhow::Result<Worksheets> {
        let state = self.worksheet.as_ref().context("Not a worksheet print")?;
        let want = &state.profile;
        let profile = Profile {
            paper: settings.paper.clone(),
            landscape: settings.landscape,
            ..want.clone()
        };
        let mut sheets = worksheet::layout(
            &state.project,
            &self.name,
            &state.panels,
            &profile,
            &state.date,
            &state.batch,
        )?;
        let same = (settings.paper.width - want.paper.width).abs() < 0.5
            && (settings.paper.height - want.paper.height).abs() < 0.5
            && settings.landscape == want.landscape;
        if !same {
            sheets.layout.warnings.push(
                t!(
                    "print.worksheets.paper_differs",
                    chosen = settings.paper.name,
                    layout = want.paper.name
                )
                .into_owned(),
            );
        }
        Ok(sheets)
    }

    /// In place of the content choice: why worksheets print at 100%.
    pub(super) fn worksheet_note(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .id("print-worksheet-note")
            .test_support()
            .text_color(theme::palette(cx).muted)
            .child(t!("print.worksheets.note"))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::super::storyboard::tests::{Host, project};
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_io::storyboard_export::{Scope, profile::builtins};
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt;

    #[gpui_kit::test]
    fn worksheets_print_through_the_dialog_with_the_pdf_geometry(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            cx.set_reduce_motion(true);
            cx.set_global(crate::app_state::AppSettings(Default::default()));
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host);
            Root::new(host, window, cx)
        });
        cx.simulate_resize(size(px(1200.), px(1000.)));
        let project = Arc::new(project());
        let six = builtins().remove(1);
        let panels = Panels::Existing(Scope::All);
        let view = cx.update(|window, cx| {
            let view = cx.new(|cx| {
                let mut dialog = PrintDialog::new("Film".into(), 0, window, cx);
                dialog.attach_worksheets(project.clone(), panels.clone(), six.clone());
                dialog
            });
            view.update(cx, |v, cx| {
                v.loading = false;
                v.choose_destination("pdf".into(), cx);
            });
            let body = view.clone();
            window.open_dialog(cx, move |dialog, _, _| dialog.child(body.clone()));
            view
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("print-worksheet-note").visible());
            assert!(
                window.try_find("print-layout").is_none(),
                "no fit or layout"
            );
            assert!(view.read(cx).preview.is_some(), "the sheets preview live");
            let (settings, layout) = view.read(cx).draft(cx).unwrap();
            assert_eq!(settings.paper.width, six.paper.width, "the layout's paper");
            assert_eq!(settings.landscape, six.landscape);
            assert!(layout.warnings.is_empty(), "{:?}", layout.warnings);
            // The print path lays out exactly the sheets Save PDF writes.
            let state = view.read(cx).worksheet.as_ref().unwrap();
            let pdf = worksheet::layout(&project, "Film", &panels, &six, &state.date, &state.batch)
                .unwrap();
            assert_eq!(layout.sheets.len(), pdf.layout.sheets.len());
            for (printed, saved) in layout.sheets.iter().zip(&pdf.layout.sheets) {
                assert_eq!((printed.width, printed.height), (saved.width, saved.height));
                let a = print::production::preview(&[], printed, &settings, 1200).unwrap();
                let b = worksheet::render(saved, 1200).unwrap();
                assert!(
                    a == b,
                    "the printed sheet matches the PDF's pixel for pixel"
                );
            }
            let dir = tempfile::tempdir().unwrap();
            let (one, two) = (dir.path().join("print.pdf"), dir.path().join("save.pdf"));
            let cancel = AtomicBool::new(false);
            print::production::write_pdf(&[], &layout, &settings, &one, &cancel).unwrap();
            print::write_pdf(&[], &pdf.layout, false, &two, &cancel).unwrap();
            // Both PDFs have the same pages at the same size.
            let boxes = |path: &std::path::Path| {
                let bytes = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
                bytes
                    .match_indices("/MediaBox [")
                    .map(|(i, _)| bytes[i..i + 40].split(']').next().unwrap().to_string())
                    .collect::<Vec<_>>()
            };
            assert!(!boxes(&one).is_empty());
            assert_eq!(boxes(&one), boxes(&two));
            // A printer with other paper: laid out again for it, at 100%,
            // with a warning.
            let letter = print::Paper {
                margins: [4.; 4],
                ..print::Paper::pdf().remove(1)
            };
            view.update(cx, |v, cx| {
                v.destination = "printer".into();
                v.caps = Some(Capabilities {
                    papers: vec![letter.clone()],
                    default_paper: letter.id.clone(),
                    ..Capabilities::pdf()
                });
                v.settings.paper = letter.clone();
                v.changed(cx);
            });
            let (settings, layout) = view.read(cx).draft(cx).unwrap();
            assert_eq!(settings.paper, letter);
            assert_eq!(layout.warnings.len(), 1);
            assert!(
                layout.warnings[0].contains("Letter"),
                "{:?}",
                layout.warnings
            );
            let sheet = &layout.sheets[0];
            let (w, h) = if settings.landscape {
                (letter.height, letter.width)
            } else {
                (letter.width, letter.height)
            };
            assert_eq!((sheet.width, sheet.height), (w, h), "full size, not scaled");
            window.close_dialog(cx);
        });
    }
}
