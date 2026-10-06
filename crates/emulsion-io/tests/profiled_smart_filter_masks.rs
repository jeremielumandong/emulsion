//! Profile routing and native round-trip use a real native Find Edges stack.
//! Photoshop final-pixel evidence lives in emulsion-core's independent fixture.
use emulsion_core::{
    Document, Node, NodeKind, SmartFilterMask,
    project::{ProjectEditor, ProjectKind},
};
use emulsion_filters::Filter;
use emulsion_io::{export, ora, pptx, selection_export};
use emulsion_raster::blend::BlendSpace;
use emulsion_raster::{Mask, Placement, Raster, composite::flatten};
use std::{io::Read, path::Path, sync::Arc};
use zip::ZipArchive;

fn fixture() -> Document {
    let mut doc = Document::new(5, 2);
    doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    // Flat black has zero gradient, so native Find Edges produces opaque white.
    // The mask-only analytic result is therefore exactly its encoded gray code.
    let mut node = Node::smart(
        1,
        "Profiled Smart mask",
        Arc::new(Raster::from_srgba8(5, 2, &[0, 0, 0, 255].repeat(10))),
        vec![Filter::FindEdges],
        Placement::default(),
    );
    let NodeKind::Smart { filter_mask, .. } = &mut node.kind else {
        unreachable!()
    };
    *filter_mask = Some(SmartFilterMask::new(Arc::new(Mask::from_fn(
        5,
        2,
        255,
        |x, _| [0, 64, 128, 192, 255][x as usize],
    ))));
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}

fn expected() -> Vec<u8> {
    [0, 64, 128, 192, 255]
        .into_iter()
        .cycle()
        .take(10)
        .flat_map(|v| [v, v, v, 255])
        .collect()
}

fn entry(path: &Path, name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut bytes = vec![];
    archive
        .by_name(name)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

#[test]
fn profiled_filter_mask_native_preview_thumbnail_and_exports_keep_encoded_mix() {
    let doc = fixture();
    let expected = expected();
    assert_eq!(flatten(&doc.composite_tree(), 0).to_srgba8(), expected);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profiled-mask.ora");
    ora::write(&doc, &path).unwrap();
    for name in [
        "mergedimage.png",
        "Thumbnails/thumbnail.png",
        "data/node-1.png",
    ] {
        let image = image::load_from_memory(&entry(&path, name))
            .unwrap()
            .to_rgba8();
        assert_eq!(image.dimensions(), (5, 2));
        assert_eq!(image.into_raw(), expected, "{name}");
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&entry(&path, "emulsion.json")).unwrap();
    assert_eq!(manifest["version"], 14);
    let reopened = ora::read(&path).unwrap();
    assert_eq!(reopened.blend_space, BlendSpace::PhotoshopSrgbV1);
    assert!(matches!(reopened.nodes[0].kind, NodeKind::Smart { .. }));
    assert_eq!(flatten(&reopened.composite_tree(), 0).to_srgba8(), expected);
    let path = dir.path().join("profiled-mask.png");
    export::export(
        &doc,
        &path,
        export::ExportOptions {
            depth: 8,
            jpeg_quality: 92,
        },
    )
    .unwrap();
    assert_eq!(image::open(&path).unwrap().to_rgba8().into_raw(), expected);
    let (selection, _) = selection_export::prepare(&doc, &[1], &Default::default()).unwrap();
    assert_eq!(selection.blend_space, doc.blend_space);
    assert_eq!(
        flatten(&selection.composite_tree(), 0).to_srgba8(),
        expected
    );
}

#[test]
fn profiled_filter_mask_powerpoint_picture_keeps_encoded_mix() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profiled-mask.pptx");
    let project = ProjectEditor::new_project(ProjectKind::Design, fixture())
        .unwrap()
        .snapshot()
        .unwrap();
    pptx::write(&project, &[project.pages[0].meta.id], &path).unwrap();
    let mut archive = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let images: Vec<_> = (0..archive.len())
        .filter_map(|i| {
            let name = archive.by_index(i).unwrap().name().to_owned();
            name.starts_with("ppt/media/").then_some(name)
        })
        .collect();
    assert_eq!(images.len(), 1);
    let image = image::load_from_memory(&entry(&path, &images[0]))
        .unwrap()
        .to_rgba8();
    assert_eq!(image.dimensions(), (5, 2));
    assert_eq!(image.into_raw(), expected());
}
