//! MCP operates on the same catalog, selection, RAW drafts and export queue as Library.
use super::*;
use anyhow::{Result, anyhow, bail};
use emulsion_io::{
    creative_library::{self as catalog, AssetKind, Catalog},
    raw::RawSource,
    raw_settings,
};
use emulsion_mcp::{
    ToolResult,
    library_tools::{self as api, Request},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

impl Workspace {
    pub(crate) fn library_mcp(
        &mut self,
        name: &str,
        args: &Value,
        cx: &mut Context<Self>,
    ) -> Task<ToolResult> {
        self.library_mcp_at(name, args, catalog::root(), cx)
    }
    pub(crate) fn library_mcp_at(
        &mut self,
        name: &str,
        args: &Value,
        root: PathBuf,
        cx: &mut Context<Self>,
    ) -> Task<ToolResult> {
        let request = match api::parse(name, args) {
            Ok(r) => r,
            Err(e) => return Task::ready(ToolResult::error(e)),
        };
        let mutating = !matches!(
            request,
            Request::State(_) | Request::Preview | Request::CancelExport
        );
        if mutating && (self.batch.mcp_busy || self.batch.running.is_some()) {
            return Task::ready(ToolResult::error(
                "Library operation/export is running; inspect get_library before retrying",
            ));
        }
        if mutating {
            self.batch.mcp_busy = true;
        }
        cx.spawn(async move |this, cx| {
            let result = run(&this, request, root, cx).await;
            if mutating {
                this.update(cx, |ws, cx| {
                    ws.batch.mcp_busy = false;
                    cx.notify();
                })
                .ok();
            }
            result.unwrap_or_else(|e| ToolResult::error(e.to_string()))
        })
    }
    fn library_mcp_state(&self, page: &api::Page, cx: &App) -> Value {
        let b = &self.batch;
        let l = &b.library;
        let active = b.current.and_then(|i| b.items.get(i)).map(|i| &i.path);
        let limit = page.limit.unwrap_or(100);
        let files:Vec<_>=b.items.iter().skip(page.offset).take(limit).map(|item|{
            let asset=l.catalog.assets.iter().find(|a|a.kind==AssetKind::Image&&a.path==item.path);
            json!({"path":item.path,"selected":item.selected,"raw":emulsion_io::raw::is_raw(&item.path),"metadata":asset,"exif":l.metadata.get(&item.path),"thumbnail":{"status":if item.thumb.is_some(){"ready"}else if b.thumbs_failed.contains_key(&item.path){"error"}else if b.thumbs_requested.contains(&item.path){"loading"}else{"pending"},"error":b.thumbs_failed.get(&item.path)}})
        }).collect();
        json!({"total":b.items.len(),"offset":page.offset,"next_offset":(page.offset.saturating_add(limit)<b.items.len()).then(||page.offset+limit),"files":files,
            "active":active,"selected":b.items.iter().filter(|i|i.selected).map(|i|&i.path).collect::<Vec<_>>(),"collections":l.catalog.collections,"catalog_revision":l.catalog.revision,
            "filters":{"query":l.search.as_ref().map(|s|s.read(cx).value().to_string()).unwrap_or_default(),"source":if l.source_paths.is_some(){"folder"}else{"all"},"collection":l.collection,"minimum_rating":l.rating,"flag":if l.flagged{"picked"}else if l.rejected{"rejected"}else{"all"},"color_label":l.color_label,"raw_only":l.raw_only,"unedited":l.unedited,"sort":if l.capture_sort{"capture_time"}else{"filename"},"reverse":l.reverse},
            "view":if b.develop.compare{"compare"}else if b.develop.before{"before"}else if b.develop.loupe{"develop"}else if b.develop.list{"list"}else{"grid"},"inspector":(["develop","info","keywords"][b.develop.inspector.min(2)]),"recipe":b.recipe,
            "develop":{"settings":active.and_then(|p|b.develop.current_params(p)),"histogram":b.develop.histogram,"histogram_kind":"32-bin display luminance","histogram_pending":b.develop.busy||b.preview.is_none(),"dirty":b.develop.dirty(),"dirty_paths":b.develop.drafts.iter().filter(|(p,v)|b.develop.saved.get(*p)!=Some(*v)).map(|(p,_)|p).collect::<Vec<_>>(),"saving":b.develop.saving,"busy":b.develop.busy,"undo_steps":active.and_then(|p|b.develop.history.get(p)).map_or(0,Vec::len)},
            "export":{"progress":b.running,"current":b.exporting,"out_dir":b.out_dir,"format":b.format},"mcp_busy":b.mcp_busy,"note":b.note.as_ref().map(|(text,error)|json!({"text":text,"error":error}))})
    }
}
fn active(ws: &Workspace) -> Result<PathBuf> {
    ws.batch
        .current
        .and_then(|i| ws.batch.items.get(i))
        .map(|i| i.path.clone())
        .ok_or_else(|| anyhow!("Select an active Library photo first"))
}
fn available(ws: &Workspace, paths: &[PathBuf]) -> Result<()> {
    for path in paths {
        if !ws.batch.items.iter().any(|i| &i.path == path)
            && !ws
                .batch
                .library
                .catalog
                .assets
                .iter()
                .any(|a| a.kind == AssetKind::Image && &a.path == path)
        {
            bail!("Unknown Library path {}; import it first", path.display())
        }
    }
    Ok(())
}
async fn settle(this: &WeakEntity<Workspace>, cx: &mut AsyncApp) -> Result<()> {
    let start = Instant::now();
    loop {
        let busy = this.update(cx, |ws, _| {
            ws.batch.develop.saving
                || ws.batch.develop.busy
                || ws.batch.library.loading
                || ws.batch.library.info_busy
                || ws.batch.running.is_some()
                || ws.busy.is_some()
        })?;
        if !busy {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(600) {
            bail!("Library operation is still running; inspect get_library before retrying")
        }
        cx.background_executor()
            .timer(Duration::from_millis(15))
            .await;
    }
}
fn check_note(ws: &Workspace) -> Result<()> {
    if let Some((text, true)) = &ws.batch.note {
        bail!("{text}")
    }
    Ok(())
}
async fn publish(this: &WeakEntity<Workspace>, catalog: Catalog, cx: &mut AsyncApp) -> Result<()> {
    this.update(cx, |ws, cx| {
        if catalog.revision >= ws.batch.library.catalog.revision {
            ws.batch.library.catalog = catalog;
        }
        ws.batch.library.loaded = true;
        ws.library_show(cx);
        cx.notify();
    })?;
    Ok(())
}
async fn run(
    this: &WeakEntity<Workspace>,
    request: Request,
    root: PathBuf,
    cx: &mut AsyncApp,
) -> Result<ToolResult> {
    if matches!(request, Request::CancelExport) {
        return this.update(cx, |ws, cx| {
            let was_running = ws.batch.running.is_some();
            if was_running {
                ws.cancel_batch(cx);
            }
            ToolResult::text(
                json!({"stopped":was_running,"in_flight_output_may_complete":was_running})
                    .to_string(),
            )
        });
    }
    // Reads remain available during long jobs so clients can inspect progress.
    if !this.update(cx, |ws, _| {
        ws.batch.library.loaded || ws.batch.library.loading
    })? {
        let load_root = root.clone();
        let catalog = cx
            .background_spawn(async move { catalog::load(&load_root) })
            .await?;
        this.update(cx, |ws, cx| {
            if catalog.revision >= ws.batch.library.catalog.revision {
                ws.batch.library.catalog = catalog;
            }
            ws.batch.library.loaded = true;
            if ws.batch.items.is_empty() {
                ws.library_show(cx);
            }
        })?;
    }
    if let Request::State(page) = request {
        return Ok(ToolResult::text(
            this.update(cx, |ws, cx| ws.library_mcp_state(&page, cx))?
                .to_string(),
        ));
    }
    if matches!(request, Request::Preview) {
        return preview(this, cx).await;
    }
    settle(this, cx).await?;
    this.update(cx, |ws, _| ws.batch.note = None)?;
    match request {
        Request::Import(import) => {
            let (folder, paths, catalog) = cx
                .background_spawn(async move {
                    let folder = import.folder.canonicalize()?;
                    if !folder.is_dir() {
                        bail!("Import requires a directory")
                    }
                    let mut paths = Vec::new();
                    for entry in std::fs::read_dir(&folder)? {
                        let path = entry?.path();
                        if path.is_file() && is_batch_input(&path) {
                            paths.push(path.canonicalize()?);
                        }
                    }
                    paths.sort();
                    if paths.is_empty() {
                        bail!("Folder contains no supported photos")
                    }
                    let (catalog, _) = catalog::update(&root, |c| {
                        for path in &paths {
                            c.add_asset(path.clone(), AssetKind::Image)?;
                        }
                        Ok(())
                    })?;
                    Ok::<_, anyhow::Error>((folder, paths, catalog))
                })
                .await?;
            this.update(cx, |ws, cx| {
                if catalog.revision >= ws.batch.library.catalog.revision {
                    ws.batch.library.catalog = catalog;
                }
                ws.batch.library.loaded = true;
                ws.batch.library.collection = None;
                ws.load_batch(folder, paths, cx);
            })?;
        }
        Request::Select(selection) => {
            this.update(cx, |ws, cx| -> Result<()> {
                for path in &selection.paths {
                    if !ws.batch.items.iter().any(|i| &i.path == path) {
                        bail!(
                            "Path is not visible in the current Library view: {}",
                            path.display()
                        )
                    }
                }
                let chosen = selection.active.as_ref().or(selection.paths.first());
                if chosen.is_some_and(|p| !selection.paths.contains(p)) {
                    bail!("active must be in paths")
                }
                for item in &mut ws.batch.items {
                    item.selected = selection.paths.contains(&item.path);
                }
                ws.batch.current =
                    chosen.and_then(|p| ws.batch.items.iter().position(|i| &i.path == p));
                ws.batch.develop.anchor = ws.batch.current;
                ws.batch.develop.before = false;
                ws.invalidate_library_preview();
                ws.batch_preview(cx);
                ws.library_read_metadata(false, cx);
                cx.notify();
                Ok(())
            })??;
            settle(this, cx).await?;
        }
        Request::View(view) => {
            let window = this.update(cx, |ws, _| ws.library_window)?;
            window.update(cx, |_, window, cx| {
                this.update(cx, |ws, cx| apply_view(ws, view, window, cx))
            })???;
            settle(this, cx).await?;
        }
        Request::Metadata(metadata) => {
            this.update(cx, |ws, _| available(ws, &metadata.paths))??;
            let catalog = cx
                .background_spawn(async move {
                    catalog::update(&root, |c| {
                        for path in metadata.paths {
                            let id = c.add_asset(path, AssetKind::Image)?;
                            let a = c.assets.iter_mut().find(|a| a.id == id).unwrap();
                            if let Some(rating) = metadata.rating {
                                a.rating = rating;
                            }
                            if let Some(label) = metadata.color_label {
                                a.color_label = label;
                            }
                            if let Some(flag) = &metadata.flag {
                                a.flagged = matches!(flag, api::Flag::Picked);
                                a.rejected = matches!(flag, api::Flag::Rejected);
                            }
                            if let Some(tags) = &metadata.keywords {
                                if !metadata.append_keywords {
                                    a.tags.clear();
                                }
                                for tag in tags {
                                    let tag = tag.trim();
                                    if !tag.is_empty() && !a.tags.iter().any(|t| t == tag) {
                                        a.tags.push(tag.into());
                                    }
                                }
                            }
                        }
                        Ok(())
                    })
                    .map(|(c, _)| c)
                })
                .await?;
            publish(this, catalog, cx).await?;
        }
        Request::Collection(collection) => {
            this.update(cx, |ws, _| available(ws, &collection.paths))??;
            let (catalog, id) = cx
                .background_spawn(async move {
                    catalog::update(&root, |c| {
                        let ids = collection
                            .paths
                            .into_iter()
                            .map(|p| c.add_asset(p, AssetKind::Image))
                            .collect::<emulsion_io::Result<Vec<_>>>()?;
                        match collection.action {
                            api::CollectionAction::Create => {
                                c.add_collection(collection.name.unwrap(), ids)
                            }
                            api::CollectionAction::Add => {
                                let group = c
                                    .collections
                                    .iter_mut()
                                    .find(|g| Some(g.id) == collection.id)
                                    .ok_or_else(|| {
                                        emulsion_io::IoError::Manifest(
                                            "Unknown collection ID".into(),
                                        )
                                    })?;
                                for id in ids {
                                    if !group.assets.contains(&id) {
                                        group.assets.push(id);
                                    }
                                }
                                Ok(group.id)
                            }
                        }
                    })
                })
                .await?;
            publish(this, catalog, cx).await?;
            return Ok(ToolResult::text(
                json!({"collection_id":id,"saved":true}).to_string(),
            ));
        }
        Request::Develop(develop) => develop_request(this, develop, cx).await?,
        Request::Export(export) => {
            let generation = this.update(cx, |ws, cx| -> Result<u64> {
                if ws.batch.develop.dirty() || ws.batch.develop.saving {
                    bail!("Save Library RAW drafts before exporting")
                }
                if !ws.batch.items.iter().any(|i| i.selected) {
                    bail!("Select photos before exporting")
                }
                ws.batch.out_dir = Some(export.out_dir);
                ws.batch.format = export.format;
                ws.run_batch(cx);
                Ok(ws.batch.run_generation)
            })??;
            settle(this, cx).await?;
            if this.update(cx, |ws, _| ws.batch.run_generation != generation)? {
                bail!("Library export was canceled; completed and in-flight outputs may exist")
            }
        }
        Request::Open => {
            let (window, path) = this.update(cx, |ws, _| -> Result<_> {
                if ws.batch.develop.dirty() {
                    bail!("Save Library RAW drafts before opening Photo")
                }
                Ok((ws.library_window, active(ws)?))
            })??;
            window.update(cx, |_, window, cx| {
                this.update(cx, |ws, cx| ws.open_photo_path(path, window, cx))
            })??;
            settle(this, cx).await?;
            return this.update(cx,|ws,_|{if let Some(error)=&ws.error{bail!("{error}")}Ok(ToolResult::text(json!({"document_id":ws.editor.as_ref().map(|e|e.entity_id().as_u64()),"opened":true}).to_string()))})?;
        }
        Request::State(_) | Request::Preview | Request::CancelExport => unreachable!(),
    }
    this.update(cx, |ws, cx| -> Result<_> {
        check_note(ws)?;
        ws.batch.mcp_busy = false;
        Ok(ToolResult::text(
            ws.library_mcp_state(&api::Page::default(), cx).to_string(),
        ))
    })?
}

fn apply_view(
    ws: &mut Workspace,
    view: api::View,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Result<()> {
    // Validate every fallible input before changing any view state.
    if let Some(id) = view.collection
        && !ws
            .batch
            .library
            .catalog
            .collections
            .iter()
            .any(|c| c.id == id)
    {
        bail!("Unknown collection ID {id}")
    }
    let folder = if matches!(view.source, Some(api::Source::Folder)) {
        Some(
            ws.batch
                .folder
                .clone()
                .ok_or_else(|| anyhow!("Import a folder first"))?,
        )
    } else {
        None
    };
    if let Some(recipe) = &view.recipe
        && !recipe.is_empty()
        && !ws
            .batch
            .recipes
            .as_ref()
            .is_some_and(|r| r.iter().any(|r| &r.name == recipe))
    {
        bail!("Unknown/unloaded recipe; use list_recipes and wait for the Library recipe catalog")
    }
    if let Some(query) = view.query {
        if ws.batch.library.search.is_none() {
            let input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Search names / keywords"));
            ws.batch.library.search_subscription =
                Some(cx.subscribe(&input, |ws, _, event, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        ws.library_show(cx);
                    }
                }));
            ws.batch.library.search = Some(input);
        }
        ws.batch
            .library
            .search
            .as_ref()
            .unwrap()
            .update(cx, |s, cx| s.set_value(query, window, cx));
    }
    let l = &mut ws.batch.library;
    if let Some(source) = view.source {
        l.collection = None;
        l.source_paths = match source {
            api::Source::All => None,
            api::Source::Folder => Some(
                l.catalog
                    .assets
                    .iter()
                    .filter(|a| a.kind == AssetKind::Image && a.path.parent() == folder.as_deref())
                    .map(|a| a.path.clone())
                    .collect(),
            ),
        };
    }
    if let Some(id) = view.collection {
        l.collection = Some(id);
        l.source_paths = None;
    }
    if let Some(rating) = view.minimum_rating {
        l.rating = rating;
    }
    if let Some(flag) = view.flag {
        l.flagged = matches!(flag, api::Flag::Picked);
        l.rejected = matches!(flag, api::Flag::Rejected);
    }
    if let Some(label) = view.color_label {
        l.color_label = label;
    }
    if let Some(raw) = view.raw_only {
        l.raw_only = raw;
    }
    if let Some(unedited) = view.unedited {
        l.unedited = unedited;
    }
    if let Some(sort) = view.sort {
        l.capture_sort = matches!(sort, api::Sort::CaptureTime);
    }
    if let Some(reverse) = view.reverse {
        l.reverse = reverse;
    }
    if let Some(mode) = view.mode {
        let d = &mut ws.batch.develop;
        d.loupe = matches!(
            mode,
            api::Mode::Develop | api::Mode::Before | api::Mode::Compare
        );
        d.list = matches!(mode, api::Mode::List);
        d.before = matches!(mode, api::Mode::Before);
        d.compare = matches!(mode, api::Mode::Compare);
    }
    if let Some(inspector) = view.inspector {
        ws.batch.develop.inspector = match inspector {
            api::Inspector::Develop => 0,
            api::Inspector::Info => 1,
            api::Inspector::Keywords => 2,
        };
    }
    if let Some(recipe) = view.recipe {
        ws.batch.recipe = (!recipe.is_empty()).then_some(recipe);
    }
    ws.library_show(cx);
    ws.batch_preview(cx);
    if ws.batch.library.capture_sort {
        ws.library_read_metadata(true, cx);
    }
    cx.notify();
    Ok(())
}

async fn develop_request(
    this: &WeakEntity<Workspace>,
    request: api::Develop,
    cx: &mut AsyncApp,
) -> Result<()> {
    use api::DevelopAction as A;
    if request.action == A::Save {
        this.update(cx, |ws, cx| ws.library_save_develop(false, cx))?;
        settle(this, cx).await?;
        this.update(cx, |ws, _| -> Result<()> {
            check_note(ws)?;
            if ws.batch.develop.dirty() {
                bail!("Some RAW drafts remain unsaved")
            }
            Ok(())
        })??;
        return Ok(());
    }
    let path = this.update(cx, |ws, _| active(ws))??;
    if !emulsion_io::raw::is_raw(&path) {
        bail!("Active Library photo is not an editable RAW")
    }
    if request.action == A::Reload {
        this.update(cx, |ws, cx| {
            ws.batch.develop.drafts.remove(&path);
            ws.batch.develop.saved.remove(&path);
            ws.batch.develop.fingerprints.remove(&path);
            ws.batch.develop.history.remove(&path);
            ws.batch.develop.source = None;
            ws.invalidate_library_preview();
            ws.batch_preview(cx);
        })?;
        settle(this, cx).await?;
        this.update(cx, |ws, _| check_note(ws))??;
        return Ok(());
    }
    this.update(cx, |ws, cx| ws.batch_preview(cx))?;
    settle(this, cx).await?;
    let (params, source) = this.update(cx, |ws, _| -> Result<_> {
        if active(ws)? != path {
            bail!("Active photo changed; retry on the current selection")
        }
        let params =
            ws.batch.develop.current_params(&path).ok_or_else(|| {
                anyhow!("RAW settings are unavailable; inspect the Library error")
            })?;
        let source = ws
            .batch
            .develop
            .source
            .clone()
            .filter(|s| s.source == path)
            .ok_or_else(|| anyhow!("RAW source is not loaded"))?;
        Ok((params, source))
    })??;
    if request.action == A::SavePreset {
        let file = request.path.unwrap();
        cx.background_spawn(async move { raw_settings::save_preset(params, &file) })
            .await?;
        return Ok(());
    }
    if request.action == A::Sync {
        this.update(cx, |ws, cx| -> Result<()> {
            if !ws
                .batch
                .items
                .iter()
                .any(|i| i.selected && emulsion_io::raw::is_raw(&i.path) && i.path != path)
            {
                bail!("Select at least one other RAW to synchronize")
            }
            ws.batch.develop.sync_group = request.group.unwrap_or(api::Group::All).raw();
            ws.library_save_develop(true, cx);
            Ok(())
        })??;
        settle(this, cx).await?;
        this.update(cx, |ws, _| check_note(ws))??;
        return Ok(());
    }
    let next = match request.action {
        A::Adjust => {
            api::patch(params, request.settings.as_ref().unwrap()).map_err(|e| anyhow!(e))?
        }
        A::Auto => {
            cx.background_spawn(async move { source.auto_adjust(&params) })
                .await?
        }
        A::Reset => raw_settings::merge_settings(
            params,
            Default::default(),
            request.group.unwrap_or(api::Group::All).raw(),
        ),
        A::AsShot => emulsion_core::raw::DevelopParams {
            wb_override: None,
            temperature: 0.,
            tint: 0.,
            ..params
        },
        A::LoadPreset => {
            let file = request.path.unwrap();
            cx.background_spawn(async move { raw_settings::load_preset(&file) })
                .await?
        }
        A::Undo => this
            .update(cx, |ws, _| {
                ws.batch
                    .develop
                    .history
                    .get(&path)
                    .and_then(|h| h.last())
                    .copied()
            })?
            .ok_or_else(|| anyhow!("No Library adjustment to undo"))?,
        A::Preset => match request.preset.unwrap() {
            api::Preset::Neutral => Default::default(),
            api::Preset::Warm => emulsion_core::raw::DevelopParams {
                temperature: 0.15,
                shadows: 0.12,
                highlights: 0.2,
                ..params
            },
            api::Preset::BlackAndWhite => emulsion_core::raw::DevelopParams {
                saturation: -1.,
                tone_curve: emulsion_core::raw::DevelopParams::MEDIUM_CONTRAST_CURVE,
                smooth_curve: true,
                ..params
            },
            api::Preset::StrongContrast => emulsion_core::raw::DevelopParams {
                tone_curve: emulsion_core::raw::DevelopParams::STRONG_CONTRAST_CURVE,
                smooth_curve: true,
                ..params
            },
        },
        A::Save | A::Reload | A::Sync | A::SavePreset => unreachable!(),
    };
    this.update(cx, |ws, cx| -> Result<()> {
        if active(ws)? != path
            || ws.batch.develop.current_params(&path) != Some(params)
            || ws.batch.develop.saving
        {
            bail!("Photo/settings changed during calculation; no adjustment applied")
        }
        if request.action == A::Undo {
            ws.batch.develop.history.get_mut(&path).unwrap().pop();
            ws.batch.develop.drafts.insert(path.clone(), next);
            ws.batch.develop.before = false;
            ws.invalidate_library_preview();
        } else {
            ws.library_adjust(next, cx);
        }
        ws.batch.develop.save_task = None;
        ws.library_save_develop(false, cx);
        cx.notify();
        Ok(())
    })??;
    settle(this, cx).await?;
    this.update(cx, |ws, _| -> Result<()> {
        check_note(ws)?;
        if ws.batch.develop.saved.get(&path) != Some(&next) {
            bail!("The requested settings were not saved; inspect Library drafts")
        }
        Ok(())
    })??;
    this.update(cx, |ws, cx| ws.batch_preview(cx))?;
    settle(this, cx).await?;
    Ok(())
}

async fn preview(this: &WeakEntity<Workspace>, cx: &mut AsyncApp) -> Result<ToolResult> {
    let (path, params, fingerprint, recipe, before, compare) =
        this.update(cx, |ws, _| -> Result<_> {
            let path = active(ws)?;
            let params = ws.batch.develop.current_params(&path);
            let fingerprint = ws.batch.develop.fingerprints.get(&path).cloned();
            Ok((
                path,
                params,
                fingerprint,
                ws.chosen_recipe(),
                ws.batch.develop.before,
                ws.batch.develop.compare,
            ))
        })??;
    cx.background_spawn(async move{
        let source=if emulsion_io::raw::is_raw(&path){Some(match fingerprint{Some(hash)=>RawSource::load_verified(&path,&hash)?,None=>RawSource::load(&path)?})}else{None};
        let params=match &source{Some(source)=>Some(match params{Some(p)=>p,None=>raw_settings::adjacent_settings(&source.source,&source.source_sha256)?}),None=>None};
        let mut result=ToolResult::text(json!({"path":path,"settings":params,"mode":if compare{"compare"}else if before{"before"}else{"edited"},"snapshot":true}).to_string());
        for original in if compare{vec![true,false]}else{vec![before]}{
            let raster=if let Some(source)=&source{
                let raster=source.develop_with(&if original{Default::default()}else{params.unwrap()})?;
                let (w,h,rgba)=super::develop::display_raster(&raster).ok_or_else(||anyhow!("Could not resize RAW preview"))?;
                Raster::from_srgba8(w,h,&rgba)
            }else{small_raster(&path,PREVIEW).ok_or_else(||anyhow!("Could not decode preview"))?};
            let (w,h,mut pixels)=render_with(Arc::new(raster),if original{None}else{recipe.as_ref()}).ok_or_else(||anyhow!("Could not render recipe preview"))?;
            for p in pixels.as_chunks_mut::<4>().0{p.swap(0,2);}
            result.content.push(json!({"type":"text","text":if original{"Before (as shot)"}else{"Edited"}}));
            result.content.push(api::png_content(w,h,&pixels).map_err(|e|anyhow!(e))?);
        }
        Ok(result)
    }).await
}
