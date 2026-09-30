use super::*;
impl Workspace {
    pub(crate) fn library_print(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.batch.develop.saving || self.batch.develop.busy || self.batch.mcp_busy {
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
        // Transfer the decoded original to the print worker. Process it first so
        // its RAW reservation is released before decoding any other selection.
        let cached = self.batch.develop.source.take();
        crate::print_dialog::open_prepared(
            "Selected Library photos".into(),
            move |cancel| {
                let mut cached_doc = match cached {
                    Some(source) => inputs
                        .iter()
                        .position(|input| input.path == source.source)
                        .map(|index| {
                            emulsion_io::printing::sources::photo_document_from_source(
                                &inputs[index],
                                &source,
                                &cancel,
                            )
                            .map(|doc| (index, doc))
                        })
                        .transpose()?,
                    None => None,
                };
                let mut sources = Vec::new();
                let mut bytes = 0;
                for (index, input) in inputs.into_iter().enumerate() {
                    emulsion_io::printing::canceled(&cancel)?;
                    let doc = if cached_doc.as_ref().is_some_and(|(i, _)| *i == index) {
                        cached_doc.take().unwrap().1
                    } else {
                        emulsion_io::printing::sources::photo_document(&input)?
                    };
                    let mut editor = Editor::new(doc, None);
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
