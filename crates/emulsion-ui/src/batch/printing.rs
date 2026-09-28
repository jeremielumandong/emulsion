use super::*;
impl Workspace {
    pub(crate) fn library_print(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.batch.develop.saving || self.batch.mcp_busy {
            self.batch.note = Some((
                "Wait for the current Library edit to finish before printing.".into(),
                true,
            ));
            cx.notify();
            return;
        }
        let inputs = self
            .batch
            .items
            .iter()
            .filter(|i| i.selected)
            .map(|i| emulsion_io::printing::sources::PhotoInput {
                path: i.path.clone(),
                params: self.batch.develop.current_params(&i.path),
                expected_digest: self.batch.develop.fingerprints.get(&i.path).cloned(),
            })
            .collect::<Vec<_>>();
        if inputs.is_empty() || inputs.len() > 200 {
            self.batch.note = Some(("Select 1–200 photos to print.".into(), true));
            cx.notify();
            return;
        }
        let recipe = self.chosen_recipe();
        crate::print_dialog::open_prepared(
            "Selected Library photos".into(),
            move |cancel| {
                let mut sources = Vec::new();
                let mut bytes = 0;
                for input in inputs {
                    emulsion_io::printing::canceled(&cancel)?;
                    let mut editor = Editor::new(
                        emulsion_io::printing::sources::photo_document(&input)?,
                        None,
                    );
                    if let Some(r) = &recipe {
                        let compiled = emulsion_recipes::compile_sized(
                            r,
                            editor.doc.width,
                            editor.doc.height,
                        )?;
                        store::add_to(&mut editor, compiled, Slot::TOP)?;
                    }
                    let name = input
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let mut source =
                        emulsion_io::printing::prepare_sources(vec![(name, editor.doc)], &cancel)?
                            .remove(0);
                    source.original_paths.push(input.path);
                    bytes += source.svg.len();
                    if bytes > 512 * 1024 * 1024 {
                        anyhow::bail!("Print sources exceed 512 MiB; select fewer photos")
                    }
                    sources.push(source);
                }
                Ok(sources)
            },
            window,
            cx,
        );
    }
}
