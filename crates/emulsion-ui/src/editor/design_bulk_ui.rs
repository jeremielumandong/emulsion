//! Generate editable local pages from CSV, keeping the source page intact.
use super::*;
use emulsion_io::design_bulk;
use gpui_kit::component::{
    Sizable, WindowExt,
    button::Button,
    input::{Textarea, TextareaState},
};

impl EditorView {
    pub(super) fn design_bulk_dialog(
        &mut self,
        initial: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let fields = match design_bulk::fields(&self.editor.doc) {
            Ok(fields) => fields,
            Err(error) => {
                self.set_status(error.to_string(), true, cx);
                return;
            }
        };
        let available =
            emulsion_core::project::MAX_PAGES.saturating_sub(self.editor.page_list().len());
        if available == 0 {
            self.set_status("This project has reached its page limit.", true, cx);
            return;
        }
        let example = || {
            format!(
                "{}\n{}",
                fields
                    .iter()
                    .map(|f| format!("\"{}\"", f.replace('"', "\"\"")))
                    .collect::<Vec<_>>()
                    .join(","),
                vec!["Example"; fields.len()].join(",")
            )
        };
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(10)
                .default_value(initial.unwrap_or_else(example))
        });
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let help = format!(
            "Fields: {}. Paste CSV with matching headers, or import a local file. Each row creates a new editable page; up to {available} pages fit. The source page is kept.",
            fields.join(", ")
        );
        window.open_dialog(cx,move|dialog,_,_|{
            let input=input.clone();let owner=owner.clone();let file_owner=owner.clone();
            dialog.title("Bulk create designs").width(px(640.))
                .child(div().flex().flex_col().gap_2().child(help.clone())
                    .child(Button::new("design-bulk-import").label("Import CSV file…").small().outline().on_click(move|_,window,cx|{
                        file_owner.update(cx,|this,cx|this.design_bulk_import(window,cx)).ok();
                    }))
                    .child(div().id("design-bulk-csv").test_support().child(Textarea::new(&input))))
                .footer(crate::widgets::form_dialog_footer("Create pages"))
                .on_ok(move|_,_,cx|{
                    let csv=input.read(cx).value().to_string();
                    owner.update(cx,|this,cx|{
                        if this.edit_ticket()!=ticket {this.set_status("The source page changed. Open bulk create again.",true,cx);return false;}
                        let source=this.editor.doc.clone();
                        this.set_status("Creating editable pages…",false,cx);
                        cx.spawn(async move|this,cx|{
                            let result=cx.background_spawn(async move{design_bulk::generate(&source,&csv,available)}).await;
                            this.update(cx,|this,cx|{
                                if this.edit_ticket()!=ticket {this.set_status("The source page changed. Generate again from the current design.",true,cx);return;}
                                match result.map_err(|e|e.to_string()).and_then(|project|this.editor.import_pages(project)) {
                                    Ok(ids)=>{this.after_change(cx);this.set_tool(Tool::Move,cx);this.set_status(format!("Created {} editable pages. Undo removes the batch.",ids.len()),false,cx);}
                                    Err(error)=>this.set_status(error,true,cx),
                                }
                            }).ok();
                        }).detach();
                        true
                    }).unwrap_or(false)
                })
        });
    }

    fn design_bulk_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ticket = self.edit_ticket();
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import local CSV".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_spawn(async move {
                    use std::io::Read;
                    let mut text = String::new();
                    std::fs::File::open(path)?
                        .take(design_bulk::MAX_CSV_BYTES as u64 + 1)
                        .read_to_string(&mut text)?;
                    if text.len() > design_bulk::MAX_CSV_BYTES {
                        return Err(std::io::Error::other(
                            "CSV files must be no larger than 2 MB.",
                        ));
                    }
                    Ok::<_, std::io::Error>(text)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                if this.edit_ticket() != ticket {
                    this.set_status("The page changed. Open bulk create again.", true, cx);
                    return;
                }
                match result {
                    Ok(text) => {
                        window.close_dialog(cx);
                        this.design_bulk_dialog(Some(text), window, cx);
                    }
                    Err(error) => this.set_status(error.to_string(), true, cx),
                }
            })
            .ok();
        })
        .detach();
    }
}
