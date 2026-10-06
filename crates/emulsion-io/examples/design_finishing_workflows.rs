//! Assert four complete, editable Design finishing workflows using public APIs.
//! cargo run --locked -p emulsion-io --example design_finishing_workflows -- NEW_DIRECTORY
//! Append --fixtures DIRECTORY to start from design_family_preview's saved .emu files.
//! Append --brief FAMILY_SLUG to isolate one brief's memory/time when debugging.
//! Run scripts/verify-design-finishing.py NEW_DIRECTORY for independent PDF checks.
//!
//! This is an IO/core regression, not a claim that the native UI was exercised.
//! Synthetic image sources are deterministic and contain no private photographs.
use anyhow::{Context, Result, ensure};
use emulsion_core::{
    Command, Document, Editor, NodeKind,
    command::{Alignment, Slot},
    design::{
        self, Element, ImageFit,
        template_families::{FAMILIES, FamilyId},
    },
    design_fonts::{self, EmbeddedFont},
    design_resize,
    layer_links::{Arrange, ArrangeTarget},
    project::{Project, ProjectEditor},
    styles::LayerStyle,
};
use emulsion_raster::{Raster, composite::flatten};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

struct Brief {
    slug: &'static str,
    family: FamilyId,
    pages: usize,
    size: (u32, u32),
    ppi: f32,
}
const BRIEFS: [Brief; 4] = [
    Brief {
        slug: "garden-vows",
        family: FamilyId::GardenVows,
        pages: 2,
        size: (1200, 1680),
        ppi: 300.,
    },
    Brief {
        slug: "field-notes",
        family: FamilyId::FieldNotes,
        pages: 3,
        size: (1080, 1350),
        ppi: 72.,
    },
    Brief {
        slug: "market-day",
        family: FamilyId::MarketDay,
        pages: 1,
        size: (1800, 2400),
        ppi: 300.,
    },
    Brief {
        slug: "studio-brief",
        family: FamilyId::StudioBrief,
        pages: 5,
        size: (1280, 720),
        ppi: 72.,
    },
];

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn snapshot(editor: &ProjectEditor) -> Result<Project> {
    editor.snapshot().context("Missing project")
}

fn same_document(before: &Document, after: &Document) -> Result<()> {
    // Native NodeKind equality deliberately compares raster buffers by Arc
    // identity for fast in-editor change detection. Reopening allocates fresh
    // buffers: assert every source sample first, then compare all node fields
    // with shared identities. This never ignores placement, clipping or styles.
    let mut comparable = after.clone();
    for node in &before.nodes {
        if let NodeKind::Raster { raster, .. } = &node.kind {
            let reopened = comparable.node_mut(node.id).context("Missing photo node")?;
            let NodeKind::Raster { raster: source, .. } = &mut reopened.kind else {
                anyhow::bail!("Editable photo was flattened or changed kind");
            };
            ensure!(
                raster.width() == source.width()
                    && raster.height() == source.height()
                    && raster.to_srgba16() == source.to_srgba16(),
                "Photo source samples changed: {}",
                node.name
            );
            *source = raster.clone();
        }
    }
    ensure!(
        before == &comparable
            && before.colors == after.colors
            && before.info == after.info
            && before.source_depth == after.source_depth
            && before.next_id == after.next_id,
        "Editable document fields changed"
    );
    Ok(())
}

fn same_project(before: &Project, after: &Project) -> Result<()> {
    ensure!(
        before.kind == after.kind
            && before.active == after.active
            && before.next_page_id == after.next_page_id
            && before.pages.len() == after.pages.len(),
        "Project identity changed"
    );
    for (a, b) in before.pages.iter().zip(&after.pages) {
        ensure!(a.meta == b.meta, "Page metadata changed: {}", a.meta.name);
        same_document(&a.doc, &b.doc)
            .with_context(|| format!("Editable page changed: {}", a.meta.name))?;
        ensure!(
            a.graph.len() == b.graph.len()
                && a.graph.head() == b.graph.head()
                && a.graph.branches() == b.graph.branches(),
            "History changed: {}",
            a.meta.name
        );
        for (a, b) in a.graph.commits().zip(b.graph.commits()) {
            ensure!(
                a.id == b.id && a.parents == b.parents,
                "Historical identity changed"
            );
            same_document(&a.doc, &b.doc).context("Historical editable source changed")?;
        }
    }
    Ok(())
}

fn round_trip(project: &Project, path: &Path) -> Result<Project> {
    emulsion_io::project::write(project, path)?;
    let reopened = emulsion_io::project::read(path)?;
    reopened.validate().map_err(anyhow::Error::msg)?;
    same_project(project, &reopened)?;
    Ok(reopened)
}

fn edit_text(editor: &mut Editor, id: u64, content: &str) -> Result<()> {
    let NodeKind::Text { spec, .. } = &editor.doc.node(id).context("Missing text")?.kind else {
        anyhow::bail!("Expected editable text");
    };
    let mut spec = spec.as_ref().clone();
    spec.text = content.into();
    editor.execute(Command::SetText {
        id,
        spec: Box::new(spec),
    })?;
    Ok(())
}

fn named_text(editor: &mut Editor, name: &str, content: &str) -> Result<()> {
    let id = editor
        .doc
        .nodes
        .iter()
        .find(|n| n.name == name && matches!(n.kind, NodeKind::Text { .. }))
        .with_context(|| format!("Missing authored text {name}"))?
        .id;
    edit_text(editor, id, content)
}

fn synthetic_photo(seed: u32) -> Arc<Raster> {
    // High-frequency detail and asymmetric colors expose source replacement,
    // crop, stretching, page ordering and accidentally flattened saves.
    // Model a real 8-bit PNG import into the templates' 8-bit documents.
    // Arbitrary linear-u16 samples would intentionally quantize on an 8-bit save.
    let pixels: Vec<u8> = (0..480u32)
        .flat_map(|y| {
            (0..720u32).flat_map(move |x| {
                let checker = if (x / 31 + y / 47 + seed).is_multiple_of(2) {
                    18
                } else {
                    0
                };
                [
                    (40 + x / 5 + seed * 7 + checker).min(255) as u8,
                    (28 + y / 3 + seed * 3).min(255) as u8,
                    (50 + (720 - x) / 6 + checker).min(255) as u8,
                    255,
                ]
            })
        })
        .collect();
    Arc::new(Raster::from_srgba8(720, 480, &pixels))
}

fn add_photo(editor: &mut Editor, area: [f64; 4], seed: u32) -> Result<u64> {
    let group = design::frame(&editor.doc, Element::Rectangle)
        .paste(editor, Slot::TOP, (0., 0.))
        .map_err(anyhow::Error::msg)?[0];
    let b = emulsion_core::geometry::node_bounds(&editor.doc, group)?
        .context("Missing frame bounds")?;
    let sx = area[2] / f64::from(b.w);
    let sy = area[3] / f64::from(b.h);
    editor.execute(Command::TransformNodes {
        ids: vec![group],
        transform: [
            sx,
            0.,
            0.,
            sy,
            area[0] - f64::from(b.x) * sx,
            area[1] - f64::from(b.y) * sy,
        ],
    })?;
    let first =
        design::place_in_frame(editor, group, synthetic_photo(seed)).map_err(anyhow::Error::msg)?;
    // Replacing a selected frame must keep its object identity and clipping.
    let replacement = synthetic_photo(seed + 1);
    let image =
        design::place_in_frame(editor, group, replacement.clone()).map_err(anyhow::Error::msg)?;
    ensure!(image == first, "Photo replacement changed node identity");
    editor.execute(
        design::fit_frame_image(&editor.doc, group, ImageFit::Cover, [0.3, 0.65])
            .map_err(anyhow::Error::msg)?,
    )?;
    let before_crop = editor.doc.clone();
    let crop = design::crop_frame_image(&editor.doc, group, [17., -13.], 1.15)
        .map_err(anyhow::Error::msg)?;
    // Canceling a trial crop is observational only.
    let mut canceled_preview = before_crop.clone();
    crop.clone().apply(&mut canceled_preview)?;
    ensure!(
        editor.doc == before_crop,
        "Canceled preview mutated the source"
    );
    editor.execute(crop)?;
    let NodeKind::Raster { raster, .. } = &editor.doc.node(image).unwrap().kind else {
        unreachable!()
    };
    ensure!(
        raster.to_srgba16() == replacement.to_srgba16(),
        "Crop changed photo source samples"
    );
    Ok(group)
}

fn arrange(editor: &mut Editor, id: u64) -> Result<()> {
    let before = editor.doc.clone();
    editor.execute(Command::TransformNodes {
        ids: vec![id],
        transform: [1., 0., 0., 1., 11., 7.],
    })?;
    ensure!(editor.doc != before, "Move did not affect selected artwork");
    ensure!(editor.undo(), "Move was not undoable");
    ensure!(editor.doc == before, "Undo did not restore arrangement");
    ensure!(editor.redo(), "Move was not redoable");
    ensure!(editor.undo(), "Repeated undo failed");
    let before = editor.doc.clone();
    editor.execute(Command::ArrangeLayers {
        ids: vec![id],
        operation: Arrange::Align(Alignment::HorizontalCenter),
        target: ArrangeTarget::Canvas,
    })?;
    let b = emulsion_core::geometry::node_bounds(&editor.doc, id)?
        .context("Missing arranged bounds")?;
    ensure!(
        (f64::from(b.x) + f64::from(b.w) / 2. - f64::from(editor.doc.width) / 2.).abs() <= 1.,
        "Canvas alignment failed"
    );
    if editor.doc != before {
        ensure!(editor.undo() && editor.doc == before, "Align undo failed");
    }
    // Exercise reordering as an explicit selection command; restore authored order.
    let parent = editor.doc.node(id).unwrap().parent;
    let position = editor
        .doc
        .children(parent)
        .iter()
        .position(|n| *n == id)
        .unwrap();
    editor.execute(Command::MoveNode {
        id,
        slot: Slot::top_of(parent),
    })?;
    editor.execute(Command::MoveNode {
        id,
        slot: Slot {
            parent,
            index: position,
        },
    })?;
    ensure!(
        editor.doc == before,
        "Reorder round trip changed editable content"
    );
    Ok(())
}

fn personalize(editor: &mut ProjectEditor, brief: &Brief) -> Result<()> {
    if brief.family == FamilyId::GardenVows {
        let details = editor
            .page_list()
            .iter()
            .find(|p| p.name.ends_with("Details"))
            .context("Missing Details")?
            .id;
        editor.remove_page(details).map_err(anyhow::Error::msg)?;
    } else if brief.family == FamilyId::StudioBrief {
        let overview = editor.page_list()[1].id;
        let copy = editor
            .duplicate_page(overview)
            .map_err(anyhow::Error::msg)?;
        editor.move_page(copy, 2).map_err(anyhow::Error::msg)?;
        let copy = editor
            .duplicate_page(overview)
            .map_err(anyhow::Error::msg)?;
        editor.move_page(copy, 3).map_err(anyhow::Error::msg)?;
    }
    ensure!(
        editor.page_list().len() == brief.pages,
        "Wrong brief page count"
    );
    let ids: Vec<_> = editor.page_list().iter().map(|p| p.id).collect();
    let font = EmbeddedFont::from_bytes(include_bytes!("../../../assets/fonts/Geist.ttf").to_vec())
        .map_err(anyhow::Error::msg)?;
    for (index, id) in ids.into_iter().enumerate() {
        editor.set_active_page(id).map_err(anyhow::Error::msg)?;
        match brief.family {
            FamilyId::GardenVows if index == 0 => {
                named_text(editor, "First name", "Morgan")?;
                named_text(editor, "Second name", "Taylor")?;
            }
            FamilyId::GardenVows => named_text(editor, "Set name", "MORGAN & TAYLOR")?,
            FamilyId::FieldNotes => {
                let (name, copy) = [
                    ("Cover headline", "A slower\nweekend."),
                    ("Story headline", "Make time\nfor the little things"),
                    ("Call to action headline", "Find your\nnext ritual."),
                ][index];
                named_text(editor, name, copy)?;
                named_text(editor, "Journal edition", "VOL. 02")?;
            }
            FamilyId::MarketDay => {
                named_text(editor, "Market title", "LOCAL\nMARKET")?;
                named_text(editor, "Market date", "SUNDAY 18 OCTOBER")?;
                named_text(editor, "Market venue", "GARDEN SQUARE")?;
            }
            FamilyId::StudioBrief => {
                let title = [
                    "A fresh\nperspective.",
                    "One clear\ndirection",
                    "A visual\nsystem",
                    "A thoughtful\nrollout",
                    "Make the\nnext move",
                ][index];
                let name = if index == 0 {
                    "Cover title"
                } else if index == 4 {
                    "Next steps title"
                } else {
                    "Overview title"
                };
                named_text(editor, name, title)?;
                named_text(
                    editor,
                    "Slide index",
                    &format!("MORGAN STUDIO  /  {:02}", index + 1),
                )?;
                let labels = [
                    "Title",
                    "Direction",
                    "Visual system",
                    "Rollout",
                    "Next steps",
                ];
                editor
                    .rename_page(id, format!("Studio Brief · {}", labels[index]), 0.)
                    .map_err(anyhow::Error::msg)?;
            }
            _ => unreachable!(),
        }
        // A portable font survives save/reopen instead of relying only on an
        // installed font with the same name. Keep the main authored faces.
        let text = editor
            .doc
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Text { .. }))
            .context("Missing editable text")?
            .id;
        design_fonts::embed(editor, &[text], font.clone()).map_err(anyhow::Error::msg)?;
        let (w, h) = (f64::from(editor.doc.width), f64::from(editor.doc.height));
        let photo = match brief.family {
            FamilyId::FieldNotes if index == 0 => Some(add_photo(
                editor,
                [w * 0.64, h * 0.62, w * 0.23, h * 0.2],
                1,
            )?),
            FamilyId::MarketDay => {
                add_photo(editor, [w * 0.09, h * 0.45, w * 0.395, h * 0.24], 2)?;
                Some(add_photo(
                    editor,
                    [w * 0.515, h * 0.45, w * 0.395, h * 0.24],
                    6,
                )?)
            }
            FamilyId::StudioBrief if index == 0 => Some(add_photo(
                editor,
                [w * 0.71, h * 0.17, w * 0.225, h * 0.47],
                9,
            )?),
            _ => None,
        };
        arrange(editor, photo.unwrap_or(text))?;
        if brief.family == FamilyId::MarketDay {
            // The supported compositor fallback remains explicit and stays at
            // document PPI; the saved source must remain text/path/photo nodes.
            editor.execute(Command::SetStyles {
                id: text,
                styles: vec![LayerStyle::OuterGlow {
                    color: [232, 181, 112],
                    opacity: 45.,
                    size: 7.,
                }],
            })?;
        }
        editor
            .create_version("Personalized finishing brief")
            .context("Could not create personalized version")?;
        editor.doc.validate()?;
    }
    Ok(())
}

fn sources(doc: &Document) -> Value {
    json!({
        "text": doc.nodes.iter().filter_map(|n| match &n.kind {
            NodeKind::Text { spec, .. } => Some(json!({"id":n.id, "name":n.name, "text":spec.text, "font":spec.font})), _ => None,
        }).collect::<Vec<_>>(),
        "photos": doc.nodes.iter().filter_map(|n| match &n.kind {
            NodeKind::Raster { raster, .. } => Some(json!({"id":n.id, "width":raster.width(), "height":raster.height(), "sha256_rgba16":digest(&raster.to_srgba16().iter().flat_map(|c| c.to_be_bytes()).collect::<Vec<_>>())})), _ => None,
        }).collect::<Vec<_>>(),
        "fonts": doc.design.fonts.iter().map(|(alias, font)| json!({"alias":alias, "sha256":digest(font.bytes())})).collect::<Vec<_>>()
    })
}

fn resize(editor: &mut ProjectEditor, brief: &Brief, out: &Path) -> Result<Vec<Value>> {
    let before = snapshot(editor)?;
    let first = before.pages[0].meta.id;
    editor.set_active_page(first).map_err(anyhow::Error::msg)?;
    let original = snapshot(editor)?;
    let plan = design_resize::prepare(&editor.doc, brief.size.0, brief.size.1, brief.ppi)
        .map_err(anyhow::Error::msg)?;
    let preview = plan.doc.clone();
    let copy = editor
        .apply_resized_page(plan.doc, true, Some("Resized copy proof".into()))
        .map_err(anyhow::Error::msg)?;
    ensure!(
        copy != first && editor.page_list().len() == brief.pages + 1,
        "Resize copy replaced original"
    );
    ensure!(
        editor.page_list()[1].id == copy && editor.doc == preview,
        "Resize copy order/content wrong"
    );
    for page in &original.pages {
        ensure!(
            editor.page(page.meta.id).unwrap().doc == page.doc,
            "Resize copy mutated an original page"
        );
    }
    round_trip(&snapshot(editor)?, &out.join("resized-copy-proof.emu"))?;
    ensure!(editor.undo(), "Resize copy cannot be undone");
    // Allocators intentionally never reuse a removed page ID.
    let restored = snapshot(editor)?;
    ensure!(
        restored.pages.len() == original.pages.len(),
        "Resize-copy undo left an extra page"
    );
    for (a, b) in restored.pages.iter().zip(&original.pages) {
        ensure!(
            a.meta == b.meta && a.doc == b.doc,
            "Resize-copy undo damaged source"
        );
    }
    ensure!(
        editor.redo() && editor.active_page() == copy && editor.doc == preview,
        "Resize-copy redo failed"
    );
    ensure!(editor.undo(), "Resize-copy second undo failed");
    let mut warnings = Vec::new();
    for page in &before.pages {
        editor
            .set_active_page(page.meta.id)
            .map_err(anyhow::Error::msg)?;
        let source = sources(&editor.doc);
        let plan = design_resize::prepare(&editor.doc, brief.size.0, brief.size.1, brief.ppi)
            .map_err(anyhow::Error::msg)?;
        warnings.push(json!({"page_id": page.meta.id, "overflow":plan.overflow, "text_reflow":plan.text_reflow,
            "text_overflow":plan.text_overflow, "photo_coverage":plan.photo_coverage, "unchecked":plan.unchecked}));
        editor
            .apply_resized_page(plan.doc, false, None)
            .map_err(anyhow::Error::msg)?;
        ensure!(
            sources(&editor.doc) == source,
            "Resize altered editable text/font/photo sources"
        );
    }
    ensure!(
        editor.page_list().len() == brief.pages,
        "Resize originals changed page count"
    );
    Ok(warnings)
}

fn prove_reopened_editability(project: &Project) -> Result<()> {
    let mut editor = ProjectEditor::open(project.clone(), None).map_err(anyhow::Error::msg)?;
    for page in &project.pages {
        editor
            .set_active_page(page.meta.id)
            .map_err(anyhow::Error::msg)?;
        let original = editor.doc.clone();
        let id = editor
            .doc
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Text { .. }))
            .unwrap()
            .id;
        edit_text(&mut editor, id, "Reopened text remains editable")?;
        ensure!(
            editor.doc != original && editor.undo() && editor.doc == original,
            "Reopened text edit/undo failed"
        );
        let frames: Vec<_> = editor
            .doc
            .nodes
            .iter()
            .filter(|n| n.is_group())
            .filter(|n| {
                design::frame_parts(&editor.doc, n.id).is_some_and(|(_, image)| image.is_some())
            })
            .map(|n| n.id)
            .collect();
        for frame in frames {
            let (_, image) = design::frame_parts(&editor.doc, frame).unwrap();
            let image = image.unwrap();
            let before = editor.doc.clone();
            let replacement = synthetic_photo(11);
            ensure!(
                design::place_in_frame(&mut editor, frame, replacement.clone())
                    .map_err(anyhow::Error::msg)?
                    == image,
                "Reopened replacement changed identity"
            );
            let NodeKind::Raster { raster, .. } = &editor.doc.node(image).unwrap().kind else {
                unreachable!()
            };
            ensure!(
                raster.to_srgba16() == replacement.to_srgba16()
                    && editor.undo()
                    && editor.doc == before,
                "Reopened photo replacement/undo failed"
            );
        }
    }
    Ok(())
}

fn prove_long_text(project: &Project, out: &Path) -> Result<Value> {
    let mut editor = ProjectEditor::open(project.clone(), None).map_err(anyhow::Error::msg)?;
    let first = project.pages[0].meta.id;
    editor.set_active_page(first).map_err(anyhow::Error::msg)?;
    let id = editor
        .doc
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Text { .. }))
        .context("Missing long-text probe target")?
        .id;
    let NodeKind::Text { spec, .. } = &editor.doc.node(id).unwrap().kind else {
        unreachable!()
    };
    let content = "Morgan and Taylor invite you to a wonderfully long weekend of food, flowers, conversation, music, and small moments worth remembering.";
    let mut spec = spec.as_ref().clone();
    spec.text = content.into();
    spec.width = Some(280.);
    spec.height = Some(35.);
    spec.size = 42.;
    editor.execute(Command::SetText {
        id,
        spec: Box::new(spec),
    })?;
    let plan = design_resize::prepare(&editor.doc, 540, 1080, 72.).map_err(anyhow::Error::msg)?;
    ensure!(
        plan.text_overflow.contains(&id) || plan.text_reflow.contains(&id),
        "A deliberately overfull paragraph was not reported by narrower resize"
    );
    let warnings =
        json!({"text_id":id, "text_overflow":plan.text_overflow, "text_reflow":plan.text_reflow});
    editor
        .apply_resized_page(plan.doc, false, None)
        .map_err(anyhow::Error::msg)?;
    let reopened = round_trip(&snapshot(&editor)?, &out.join("long-text-resize-proof.emu"))?;
    let doc = &reopened
        .pages
        .iter()
        .find(|p| p.meta.id == first)
        .unwrap()
        .doc;
    let NodeKind::Text { spec, .. } = &doc.node(id).unwrap().kind else {
        anyhow::bail!("Long text was flattened");
    };
    ensure!(spec.text == content, "Long content was silently truncated");
    Ok(json!({"exact_content_preserved":true, "warning_reported":true, "warnings":warnings}))
}

fn export(project: &Project, selected: &[u64], out: &Path, stem: &str) -> Result<Value> {
    let pristine = project.clone();
    let png_path = out.join(format!("{stem}.zip"));
    let pdf_path = out.join(format!("{stem}.pdf"));
    let png = emulsion_io::project_export::write(
        project,
        selected,
        emulsion_io::project_export::Format::Png,
        false,
        &png_path,
    )?;
    let pdf = emulsion_io::project_export::write(
        project,
        selected,
        emulsion_io::project_export::Format::Pdf,
        false,
        &pdf_path,
    )?;
    ensure!(
        png.pages == selected.len() && pdf.pages == selected.len(),
        "Wrong export page count"
    );
    let pages: Vec<_> = project
        .pages
        .iter()
        .enumerate()
        .filter(|(_, p)| selected.contains(&p.meta.id))
        .collect();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&png_path)?)?;
    ensure!(zip.len() == pages.len(), "Wrong PNG archive count");
    let mut manifest = Vec::new();
    for (index, (position, page)) in pages.into_iter().enumerate() {
        let mut entry = zip.by_index(index)?;
        ensure!(
            entry.name() == format!("page-{:03}-{}.png", position + 1, page.meta.id),
            "PNG output order changed"
        );
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        let image = image::load_from_memory(&bytes)?.into_rgba16();
        ensure!(
            image.dimensions() == (page.doc.width, page.doc.height),
            "PNG dimensions changed"
        );
        ensure!(
            image.into_raw() == flatten(&page.doc.composite_tree(), 0).to_srgba16(),
            "PNG samples differ from final native page"
        );
        let name = format!("page-{:02}.png", position + 1);
        if stem == "all-pages" {
            std::fs::write(out.join(&name), &bytes)?;
        }
        manifest.push(json!({"id":page.meta.id, "name":page.meta.name, "position":position + 1,
            "png":name, "width":page.doc.width, "height":page.doc.height, "ppi":page.doc.resolution,
            "pdf_points":[page.doc.width as f32 * 72. / page.doc.resolution, page.doc.height as f32 * 72. / page.doc.resolution]}));
    }
    for (name, ppi) in pdf.rasterized_pages.iter().zip(&pdf.rasterized_page_ppi) {
        let page = project
            .pages
            .iter()
            .find(|p| &p.meta.name == name)
            .context("Unknown fallback page")?;
        ensure!(*ppi == page.doc.resolution, "PDF fallback PPI changed");
    }
    same_project(&pristine, project)?;
    Ok(
        json!({"pdf":format!("{stem}.pdf"), "png_archive":format!("{stem}.zip"), "pages":manifest,
        "rasterized_pages":pdf.rasterized_pages, "rasterized_page_ppi":pdf.rasterized_page_ppi,
        "rasterized_effect_pages":pdf.rasterized_effect_pages, "exact_png_samples":true, "source_unchanged":true}),
    )
}

fn run(brief: &Brief, root: &Path, fixtures: Option<&Path>) -> Result<Value> {
    let out = root.join(brief.slug);
    std::fs::create_dir(&out)?;
    let input = fixtures.map(|path| path.join(brief.slug).join("selected-set.emu"));
    let original_bytes = input.as_ref().map(std::fs::read).transpose()?;
    let project = if let Some(path) = &input {
        emulsion_io::project::read(path)?
    } else {
        FAMILIES
            .iter()
            .find(|f| f.id == brief.family)
            .context("Missing family")?
            .selection()
            .create()
            .map_err(anyhow::Error::msg)?
    };
    let mut editor = ProjectEditor::open(project, None).map_err(anyhow::Error::msg)?;
    personalize(&mut editor, brief)?;
    round_trip(&snapshot(&editor)?, &out.join("personalized-original.emu"))?;
    let long_text = if brief.family == FamilyId::FieldNotes {
        Some(prove_long_text(&snapshot(&editor)?, &out)?)
    } else {
        None
    };
    let warnings = resize(&mut editor, brief, &out)?;
    let final_path = out.join("finished.emu");
    let project = round_trip(&snapshot(&editor)?, &final_path)?;
    let saved_bytes = std::fs::read(&final_path)?;
    prove_reopened_editability(&project)?;
    let ids: Vec<_> = project.pages.iter().map(|p| p.meta.id).collect();
    let mut selected = vec![*ids.last().unwrap()];
    if ids.len() > 1 {
        selected.push(ids[0]);
    }
    let all = export(&project, &ids, &out, "all-pages")?;
    let selected = export(&project, &selected, &out, "selected-pages")?;
    if brief.family == FamilyId::MarketDay {
        ensure!(
            all["rasterized_pages"].as_array().unwrap().len() == 1
                && all["rasterized_page_ppi"] == json!([300.]),
            "Expected glow fallback at native print PPI"
        );
    }
    ensure!(
        saved_bytes == std::fs::read(&final_path)?,
        "Export overwrote finished source"
    );
    if let (Some(path), Some(bytes)) = (&input, &original_bytes) {
        ensure!(*bytes == std::fs::read(path)?, "Input fixture was modified");
    }
    let source_manifest: Vec<_> = project
        .pages
        .iter()
        .map(|p| json!({"id":p.meta.id, "editable_sources":sources(&p.doc)}))
        .collect();
    let result = json!({"brief":brief.slug, "expected_pages":brief.pages, "final_project":"finished.emu",
        "final_sha256":digest(&saved_bytes), "input_unchanged":true, "round_trip_exact":true,
        "reopened_edit_undo_passed":true, "resize_copy_undo_redo_passed":true,
        "resize_original_source_preservation_passed":true, "resize_warnings":warnings,
        "sources":source_manifest, "long_text_resize":long_text, "exports":[all, selected]});
    std::fs::write(
        out.join("verification.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!(
        "{}: {} finished pages; editable round-trip, resize copy/original, PNG samples and export invariants passed",
        brief.slug, brief.pages
    );
    Ok(result)
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let out = PathBuf::from(args.next().context("Pass a new output directory")?);
    let mut fixtures = None;
    let mut only = None;
    while let Some(flag) = args.next() {
        if flag == "--fixtures" {
            ensure!(fixtures.is_none(), "Repeated --fixtures");
            fixtures = Some(PathBuf::from(
                args.next().context("Missing fixture directory")?,
            ));
        } else if flag == "--brief" {
            ensure!(only.is_none(), "Repeated --brief");
            only = Some(
                args.next()
                    .context("Missing brief slug")?
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("Invalid brief slug"))?,
            );
        } else {
            anyhow::bail!("Expected --fixtures DIRECTORY or --brief FAMILY_SLUG");
        }
    }
    ensure!(
        only.as_ref()
            .is_none_or(|slug| BRIEFS.iter().any(|b| b.slug == slug)),
        "Unknown brief slug"
    );
    // Refuse to overwrite a previous run or any user files.
    std::fs::create_dir(&out)?;
    let mut report = Vec::new();
    for brief in BRIEFS
        .iter()
        .filter(|brief| only.as_ref().is_none_or(|slug| brief.slug == slug))
    {
        report.push(run(brief, &out, fixtures.as_deref()).with_context(|| brief.slug)?);
    }
    std::fs::write(
        out.join("verification.json"),
        serde_json::to_vec_pretty(&json!({"briefs":report,
        "scope":"Public core/IO APIs; native UI requires separate audit", "photo_source":"Deterministic synthetic raster fixtures"}))?,
    )?;
    println!(
        "Requested finishing briefs passed. Run scripts/verify-design-finishing.py for independent PDF validation."
    );
    Ok(())
}
