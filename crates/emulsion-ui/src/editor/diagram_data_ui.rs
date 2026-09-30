//! Local, validated graph generation and explicit data refresh.
use super::*;
use emulsion_io::diagram_data::{self, Format};
use gpui_kit::component::{
    WindowExt,
    input::{Textarea, TextareaState},
};
impl EditorView {
    pub(super) fn diagram_data_dialog(
        &mut self,
        format: Format,
        refresh: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let initial = match format {
            Format::Text => "Start\nReview\nPublish",
            Format::Csv => "id,label,type,next,owner\na,Start,process,b,\nb,Ready?,decision,,",
            Format::Mermaid => "flowchart TD\nA[Start] --> B{Ready?}\nB -->|Yes| C((Done))",
            Format::D2 => "direction: right\nstart: Start\nreview: Review\nstart -> review: Submit",
            Format::Graphviz => {
                "digraph {\nrankdir=LR;\nstart [label=\"Start\"];\nstart -> review [label=\"Submit\"];\n}"
            }
            Format::Sql => {
                "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);\nCREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER REFERENCES users(id));"
            }
        };
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(12)
                .default_value(if refresh { "" } else { initial })
        });
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let help = if refresh {
            "Refresh updates labels and data matched by source_id. Positions and existing connections are retained; import a new page to change structure."
        } else {
            match format {
                Format::Text => {
                    "One step per line creates a sequence. Use A -> B -> C for explicit branches."
                }
                Format::Csv => {
                    "Required id; optional label, type, next (semicolon-separated IDs), edge_label. Other columns become local data fields."
                }
                Format::Mermaid => {
                    "Import flowcharts, states, sequences, classes, ER, mind maps and Sankey data. Other Mermaid families become editable source notes. Review compatibility notes after import."
                }
                Format::D2 => {
                    "Nodes, labels, connections and nested containers become editable shapes. Containers are flattened; styles are retained as data. External imports are not evaluated."
                }
                Format::Graphviz => {
                    "Import DOT graph or digraph nodes, attributes and connections. Emulsion supplies the layout; HTML labels require an SVG export."
                }
                Format::Sql => {
                    "CREATE TABLE statements, columns and REFERENCES foreign keys. SQL is read as a schema and never executed."
                }
            }
        };
        window.open_dialog(cx, move |dialog, window, _| {
            let input = input.clone();
            let owner = owner.clone();
            dialog
                .title(if refresh {
                    "Refresh diagram from CSV".into()
                } else {
                    format!("Create page from {}", format.label())
                })
                .width(px(960.))
                .child(
                    div().flex().flex_col().gap_3().child(help).child(
                        div()
                            .id("diagram-data-source")
                            .test_support()
                            .flex_shrink_0()
                            .child(
                                Textarea::new(&input)
                                    .h(px((f32::from(window.viewport_size().height) * 0.5)
                                        .clamp(240., 520.)))
                                    .aria_label("Diagram source"),
                            ),
                    ),
                )
                .footer(crate::widgets::form_dialog_footer("Apply"))
                .on_ok(move |_, _, cx| {
                    let text = input.read(cx).value().to_string();
                    owner
                        .update(cx, |this, cx| {
                            if this.edit_ticket() != ticket {
                                this.set_status(
                                    "The page changed. Open generation again.",
                                    true,
                                    cx,
                                );
                                return false;
                            }
                            match diagram_data::parse(&text, format) {
                                Ok(draft) => {
                                    if refresh {
                                        match draft.refresh_commands(&this.editor.doc) {
                                            Ok(commands) => {
                                                this.execute_layer_commands(
                                                    "Refresh diagram data",
                                                    commands,
                                                    cx,
                                                );
                                                true
                                            }
                                            Err(e) => {
                                                this.set_status(e.to_string(), true, cx);
                                                false
                                            }
                                        }
                                    } else {
                                        this.install_diagram_draft(
                                            draft,
                                            format!("{} diagram", format.label()),
                                            cx,
                                        );
                                        true
                                    }
                                }
                                Err(e) => {
                                    this.set_status(e.to_string(), true, cx);
                                    false
                                }
                            }
                        })
                        .unwrap_or(false)
                })
        });
    }
    fn install_diagram_draft(
        &mut self,
        draft: diagram_data::Draft,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let ticket = self.edit_ticket();
        let shapes = draft.items.len();
        let edges = draft.links.len();
        let warnings = draft.warnings.clone();
        self.set_status(
            format!("Building {shapes} shapes and {edges} connections…"),
            false,
            cx,
        );
        cx.spawn(async move|this,cx|{
            let result=cx.background_spawn(async move{draft.document()}).await;
            this.update(cx,|this,cx|{
                if this.edit_ticket()!=ticket{this.set_status("The project changed while the draft was built. Generate again on the intended page.",false,cx);return;}
                match result.map_err(|e|e.to_string()).and_then(|doc|this.editor.add_page(doc,name,0.)){
                    Ok(_)=>{this.diagram_import_notes(warnings.clone());this.after_change(cx);this.set_tool(Tool::Move,cx);let notes=if warnings.is_empty(){String::new()}else{format!(" {} compatibility note(s); review Import / export notes.",warnings.len())};this.set_status(format!("Created {shapes} editable shapes and {edges} connections on a new page.{notes}"),!warnings.is_empty(),cx);},Err(e)=>this.set_status(e,true,cx)
                }
            }).ok();
        }).detach();
    }
    pub(super) fn diagram_conditional_fill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(model) = &self.editor.doc.diagram else {
            return;
        };
        let selected = self.selected_layer_roots();
        let ids = model
            .shapes
            .keys()
            .filter(|id| {
                selected.is_empty()
                    || selected
                        .iter()
                        .any(|root| root == *id || self.editor.doc.is_ancestor(*root, **id))
            })
            .copied()
            .collect::<Vec<_>>();
        if ids.is_empty() {
            self.set_status(
                "Select a diagram shape, or clear the selection to style every shape.",
                true,
                cx,
            );
            return;
        }
        let fields = ["status", "done", "#45A477"]
            .map(|value| cx.new(|cx| InputState::new(window, cx).default_value(value)));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx,move|dialog,_,_|{let inputs=fields.clone();let owner=owner.clone();let ids=ids.clone();
            dialog.title("Color shapes by data").width(px(430.)).child(div().flex().flex_col().gap_2().child("The rule is saved on selected shapes and follows data refreshes. Later matching rules take precedence.").children(["Data field","Equals","Fill · #RRGGBB"].into_iter().zip(&fields).map(|(label,input)|div().child(label).child(Input::new(input))))).footer(crate::widgets::form_dialog_footer("Apply rule"))
.on_ok(move|_,_,cx|{
                let values=inputs.each_ref().map(|i|i.read(cx).value().to_string());let hex=values[2].trim().trim_start_matches('#');if hex.len()!=6{return false;}let Ok(rgb)=u32::from_str_radix(hex,16)else{return false;};
                owner.update(cx,|this,cx|{if this.edit_ticket()!=ticket{return false;}let Some(mut model)=this.editor.doc.diagram.as_deref().cloned()else{return false;};
                    for id in &ids {if let Some(shape)=model.shapes.get_mut(id){shape.conditions.push(emulsion_core::diagram::ConditionalFill{field:values[0].trim().into(),equals:values[1].clone(),color:[(rgb>>16)as u8,(rgb>>8)as u8,rgb as u8,255]});}}
                    match model.validate(&this.editor.doc){Ok(())=>{this.execute(Command::SetDiagram{diagram:Some(Arc::new(model))},cx);true},Err(e)=>{this.set_status(e,true,cx);false}}
                }).unwrap_or(false)
            })
        });
    }
    pub(super) fn clear_diagram_conditions(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let selected = self.selected_layer_roots();
        let Some(mut model) = self.editor.doc.diagram.as_deref().cloned() else {
            return;
        };
        for (id, shape) in &mut model.shapes {
            if selected.is_empty()
                || selected
                    .iter()
                    .any(|root| root == id || self.editor.doc.is_ancestor(*root, *id))
            {
                shape.conditions.clear();
            }
        }
        self.execute(
            Command::SetDiagram {
                diagram: Some(Arc::new(model)),
            },
            cx,
        );
    }
    pub(super) fn import_diagram_data(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import CSV, SQL, Mermaid, D2, Graphviz or text".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let ticket = this
                .update(cx, |this, cx| {
                    this.prepare_page_action(cx).then(|| this.edit_ticket())
                })
                .ok()
                .flatten();
            let Some(ticket) = ticket else {
                return;
            };
            let result: emulsion_io::Result<_> = cx
                .background_spawn(async move {
                    use std::io::Read;
                    let format = Format::from_extension(
                        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
                    )
                    .ok_or_else(|| {
                        emulsion_io::IoError::Manifest(
                            "Choose CSV, SQL, Mermaid, D2, Graphviz or text.".into(),
                        )
                    })?;
                    let mut text = String::new();
                    std::fs::File::open(&path)?
                        .take((1 << 20) + 1)
                        .read_to_string(&mut text)?;
                    let name = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(180)
                        .collect::<String>();
                    Ok((
                        diagram_data::parse(&text, format)?,
                        if name.is_empty() {
                            "Imported diagram".into()
                        } else {
                            name
                        },
                    ))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.edit_ticket() != ticket {
                    this.set_status(
                        "The page changed while importing. Import the data again.",
                        false,
                        cx,
                    );
                    return;
                }
                match result {
                    Ok((draft, name)) => this.install_diagram_draft(draft, name, cx),
                    Err(e) => this.set_status(e.to_string(), true, cx),
                }
            })
            .ok();
        })
        .detach();
    }
}
