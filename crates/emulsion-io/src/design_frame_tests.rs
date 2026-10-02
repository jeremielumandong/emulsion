use base64::Engine as _;
use emulsion_core::{
    Command, Document, Editor, NodeKind,
    command::Slot,
    design::{Element, crop_frame_image, frame, frame_parts, place_in_frame},
    project::{Project, ProjectEditor, ProjectKind},
};
use emulsion_raster::{Raster, composite::flatten};
use std::{
    io::{Cursor, Read},
    sync::Arc,
};

// The PDF writer stores direct stream lengths. Compare the emitted image and
// drawing streams rather than the whole file: PDF resource dictionaries may
// be serialized in a different HashMap order without changing the artwork.
fn pdf_streams(bytes: &[u8]) -> Vec<Vec<u8>> {
    let marker = b"\nstream\n";
    let mut cursor = 0;
    let mut streams = Vec::new();
    while let Some(offset) = bytes[cursor..]
        .windows(marker.len())
        .position(|window| window == marker)
    {
        let start = cursor + offset;
        let dictionary = String::from_utf8_lossy(&bytes[cursor..start]);
        let length = dictionary
            .rsplit("/Length ")
            .next()
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let data = start + marker.len();
        streams.push(bytes[data..data + length].to_vec());
        cursor = data + length;
    }
    assert!(!streams.is_empty(), "The PDF must contain exported artwork");
    streams.sort();
    streams
}

fn assert_png_and_pdf_roundtrip(original: &Project, restored: &Project) {
    use crate::project_export::{Format, write};
    let directory = tempfile::tempdir().unwrap();
    let expected = flatten(&original.pages[0].doc.composite_tree(), 0).to_srgba16();
    let mut original_pdf = None;
    for (name, project) in [("original", original), ("reopened", restored)] {
        let id = project.pages[0].meta.id;
        let path = directory.path().join(format!("{name}.zip"));
        let report = write(project, &[id], Format::Png, false, &path).unwrap();
        assert_eq!(report.pages, 1);
        let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        assert_eq!(archive.len(), 1);
        let mut bytes = Vec::new();
        archive
            .by_index(0)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let png = image::load_from_memory(&bytes).unwrap().to_rgba16();
        assert_eq!(png.dimensions(), (320, 240));
        assert_eq!(png.as_raw(), &expected);

        let path = directory.path().join(format!("{name}.pdf"));
        let report = write(project, &[id], Format::Pdf, false, &path).unwrap();
        assert_eq!(report.pages, 1);
        assert!(report.rasterized_pages.is_empty());
        let bytes = std::fs::read(path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 1"));
        assert!(text.contains("/TrimBox"));
        assert!(text.contains("/BleedBox"));
        let streams = pdf_streams(&bytes);
        if let Some(expected) = &original_pdf {
            assert_eq!(&streams, expected, "PDF artwork changes after reopening");
        } else {
            original_pdf = Some(streams);
        }
    }
}

#[test]
fn replacement_and_crop_roundtrip_keep_original_pixels_geometry_and_native_export() {
    let mut editor = Editor::new(Document::new(320, 240), None);
    let group = frame(&editor.doc, Element::Rectangle)
        .paste(&mut editor, Slot::TOP, (0., 0.))
        .unwrap()[0];
    let boundary = frame_parts(&editor.doc, group).unwrap().0;
    let image = place_in_frame(
        &mut editor,
        group,
        Arc::new(Raster::solid(120, 80, [0.2, 0.3, 0.4, 1.])),
    )
    .unwrap();
    editor
        .execute(Command::RotateNode {
            id: group,
            degrees: 27.,
        })
        .unwrap();
    let NodeKind::Raster { placement, .. } = &mut editor.doc.node_mut(image).unwrap().kind else {
        panic!("Expected image")
    };
    placement.flip_x = true;
    let geometry = editor.doc.node(boundary).unwrap().clone();
    let pixels = (0..64 * 24)
        .map(|i| [(i % 64 * 1000) as u16, (i / 64 * 2700) as u16, 12000, 65535])
        .collect::<Vec<_>>();
    let source = Arc::new(Raster::from_pixels(64, 24, [0; 4], &pixels));
    assert_eq!(
        place_in_frame(&mut editor, group, source.clone()).unwrap(),
        image
    );
    editor
        .execute(crop_frame_image(&editor.doc, group, [15., -9.], 1.8).unwrap())
        .unwrap();
    assert_eq!(editor.doc.node(boundary), Some(&geometry));
    let original = editor.doc.clone();
    let rendered = flatten(&original.composite_tree(), 0).to_srgba16();
    let (svg_before, flattened) = crate::project_export::svg(&original).unwrap();
    assert!(!flattened);

    let project = ProjectEditor::new_project(ProjectKind::Design, original.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    let mut archive = Cursor::new(Vec::new());
    crate::project::write_to(&project, &mut archive).unwrap();
    let mut restored = crate::project::read_from(Cursor::new(archive.into_inner())).unwrap();
    assert_png_and_pdf_roundtrip(&project, &restored);
    let doc = &mut restored.pages[0].doc;
    assert_eq!(frame_parts(doc, group), Some((boundary, Some(image))));
    assert_eq!(doc.node(boundary), Some(&geometry));
    assert_eq!(flatten(&doc.composite_tree(), 0).to_srgba16(), rendered);
    let (svg_after, flattened) = crate::project_export::svg(doc).unwrap();
    assert!(
        !flattened,
        "The frame and crop must retain native SVG geometry"
    );
    assert_eq!(svg_after, svg_before);
    let svg = String::from_utf8(svg_after).unwrap();
    assert_eq!(svg.matches("<image ").count(), 1);
    assert!(svg.contains("<image width=\"64\" height=\"24\" transform=\"matrix("));
    assert!(svg.contains(&format!("clip-path=\"url(#clip-{image})\"")));
    let NodeKind::Path { path, .. } = &geometry.kind else {
        panic!("Expected frame boundary")
    };
    assert!(svg.contains(&format!("<path d=\"{}\"/>", path.to_svg())));
    let embedded = svg
        .split("data:image/png;base64,")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let png = base64::engine::general_purpose::STANDARD
        .decode(embedded)
        .unwrap();
    let exported = image::load_from_memory(&png).unwrap().to_rgba16();
    assert_eq!(exported.dimensions(), (64, 24));
    assert_eq!(exported.as_raw(), &source.to_srgba16());
    resvg::usvg::Tree::from_str(&svg, &Default::default()).unwrap();

    let NodeKind::Raster { raster, placement } = &mut doc.node_mut(image).unwrap().kind else {
        panic!("Expected reopened image")
    };
    let NodeKind::Raster {
        placement: expected,
        ..
    } = &original.node(image).unwrap().kind
    else {
        panic!("Expected original image")
    };
    assert_eq!(placement, expected);
    assert_eq!((raster.width(), raster.height()), (64, 24));
    for y in 0..24 {
        for x in 0..64 {
            assert_eq!(raster.get(x, y), source.get(x, y));
        }
    }
    // Document equality uses raster identity; normalize it only after checking
    // every original pixel and the exact native placement above.
    *raster = source;
    assert_eq!(doc, &original);
    assert_eq!(
        editor.doc, original,
        "Saving and export cannot mutate the editor"
    );

    let mut reopened = Editor::new(doc.clone(), None);
    let before = reopened.doc.clone();
    reopened
        .execute(crop_frame_image(&reopened.doc, image, [-20., 10.], 1.2).unwrap())
        .unwrap();
    assert_eq!(reopened.doc.node(boundary), Some(&geometry));
    assert!(reopened.undo());
    assert_eq!(reopened.doc, before);
}
