//! Local, self-contained browser output uses the same native project snapshot.
use super::*;
use gpui_kit::component::WindowExt;
fn parse_widths(value: &str) -> Result<Vec<u32>, String> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let widths = value
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<u32>()
                .map_err(|_| "Enter comma-separated whole pixel widths.".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if widths.len() > 16
        || widths.iter().any(|w| !(64..=8192).contains(w))
        || widths
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != widths.len()
    {
        return Err("Choose up to 16 unique widths between 64 and 8192 pixels.".into());
    }
    Ok(widths)
}
impl EditorView {
    pub(super) fn export_design_html(
        &mut self,
        all: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(project) = self.editor.snapshot() else {
            return;
        };
        let pages = if all {
            project.pages.iter().map(|p| p.meta.id).collect()
        } else {
            vec![project.active]
        };
        let widths = cx.new(|cx| InputState::new(window, cx));
        let error = cx.new(|_| String::new());
        let owner = cx.weak_entity();
        window.open_dialog(cx,move|dialog,_,cx| {
            let widths=widths.clone();let error_apply=error.clone();let owner=owner.clone();let project=project.clone();let pages=pages.clone();
            dialog.title("Export interactive HTML").width(px(470.))
                .child(div().flex().flex_col().gap_2().child("Responsive widths · comma-separated pixels")
                    .child(Input::new(&widths).id("html-export-widths"))
                    .child("Leave blank for phone, tablet and authored breakpoints. The original page width is always included. Layout is sampled at these widths and scales between them.")
                    .child("The file embeds native outlined text and local media. YouTube playback requires internet access. Fullscreen hides controls; use arrow keys to navigate.")
                    .child(error.read(cx).clone()))
                .footer(crate::widgets::form_dialog_footer("Choose file…"))
                .on_ok(move|_,_,cx|match parse_widths(widths.read(cx).value().as_ref()) {
                    Ok(widths)=> { owner.update(cx,|this,cx|this.choose_html_export(project.clone(),pages.clone(),widths,cx)).ok();true },
                    Err(message)=> { error_apply.update(cx,|value,cx|{*value=message;cx.notify();});false }
                })
        });
    }
    fn choose_html_export(
        &mut self,
        project: emulsion_core::project::Project,
        pages: Vec<u64>,
        widths: Vec<u32>,
        cx: &mut Context<Self>,
    ) {
        let dir = self
            .editor
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(PathBuf::from)
            .unwrap_or_else(|| ".".into());
        let name = format!("{}-presentation.html", self.name);
        let receiver = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |owner, cx| {
            let Ok(Ok(Some(mut path))) = receiver.await else {
                return;
            };
            path.set_extension("html");
            let output = path.clone();
            owner
                .update(cx, |this, cx| {
                    this.set_status("Exporting interactive HTML…", false, cx)
                })
                .ok();
            let result = cx
                .background_spawn(async move {
                    emulsion_io::design_html::write(&project, &pages, &widths, &output)
                })
                .await;
            owner
                .update(cx, |this, cx| match result {
                    Ok(report) => this.set_status(
                        format!(
                            "Exported {} pages in {} responsive views to {}. {}",
                            report.pages,
                            report.views,
                            path.display(),
                            report.warnings.join(" ")
                        ),
                        false,
                        cx,
                    ),
                    Err(error) => this.set_status(format!("HTML export failed: {error}"), true, cx),
                })
                .ok();
        })
        .detach();
    }
}
#[cfg(test)]
mod tests {
    use super::parse_widths;
    #[test]
    fn design_html_widths_reject_ambiguous_invalid_and_unbounded_values() {
        assert_eq!(parse_widths("375, 768,1280").unwrap(), vec![375, 768, 1280]);
        assert!(parse_widths("").unwrap().is_empty());
        for bad in ["0", "63", "8193", "375,375", "10.5", "NaN", "768,"] {
            assert!(parse_widths(bad).is_err(), "{bad}");
        }
    }
}
