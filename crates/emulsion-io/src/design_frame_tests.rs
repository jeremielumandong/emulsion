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
    assert_png_and_pdf_roundtrip_with_fallback(original, restored, false);
}

fn assert_png_and_pdf_roundtrip_with_fallback(
    original: &Project,
    restored: &Project,
    expected_fallback: bool,
) {
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
        assert_eq!(!report.rasterized_pages.is_empty(), expected_fallback);
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

#[test]
fn page_background_roundtrip_preserves_roles_pixels_crop_and_faithful_exports() {
    use emulsion_core::{Node, design_background as background};
    let mut editor = Editor::new(Document::new(320, 240), None);
    background::set_color(&mut editor, [80, 40, 20, 255]).unwrap();
    let source = Arc::new(Raster::from_fn(96, 40, [0; 4], |x, y| {
        let alpha = if x < 32 {
            0
        } else if x < 64 {
            32768
        } else {
            65535
        };
        [
            u16::min(x as u16 * 500, alpha),
            u16::min(y as u16 * 1000, alpha),
            0,
            alpha,
        ]
    }));
    let image = editor
        .execute(Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Original page photograph",
                source.clone(),
                emulsion_raster::Placement {
                    rotation: 17.,
                    flip_x: true,
                    flip_y: true,
                    ..Default::default()
                },
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    background::set_image(&mut editor, image).unwrap();
    editor
        .execute(crop_frame_image(&editor.doc, image, [13., -7.], 1.6).unwrap())
        .unwrap();
    let original = editor.doc.clone();
    let role = background::parts(&original).unwrap();
    let (svg_before, flattened) = crate::project_export::svg(&original).unwrap();
    assert!(
        flattened,
        "Invisible page boundaries use the faithful existing compositor fallback"
    );
    let project = ProjectEditor::new_project(ProjectKind::Design, original.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    let mut archive = Cursor::new(Vec::new());
    crate::project::write_to(&project, &mut archive).unwrap();
    let restored = crate::project::read_from(Cursor::new(archive.into_inner())).unwrap();
    assert_png_and_pdf_roundtrip_with_fallback(&project, &restored, true);
    let doc = &restored.pages[0].doc;
    assert_eq!(background::parts(doc), Some(role));
    assert_eq!(background::color(doc), [80, 40, 20, 255]);
    assert_eq!(
        frame_parts(doc, role.image.unwrap().group),
        Some((role.image.unwrap().boundary, Some(image)))
    );
    let NodeKind::Raster { raster, placement } = &doc.node(image).unwrap().kind else {
        panic!()
    };
    let NodeKind::Raster {
        placement: expected,
        ..
    } = &original.node(image).unwrap().kind
    else {
        panic!()
    };
    assert_eq!(placement, expected);
    assert_eq!((raster.width(), raster.height()), (96, 40));
    for y in 0..40 {
        for x in 0..96 {
            assert_eq!(raster.get(x, y), source.get(x, y));
        }
    }
    let (svg_after, flattened) = crate::project_export::svg(doc).unwrap();
    assert!(flattened);
    assert_eq!(svg_after, svg_before);
    let svg = String::from_utf8(svg_after).unwrap();
    assert_eq!(svg.matches("<image ").count(), 1);
    assert!(svg.contains("<image width=\"320\" height=\"240\" transform=\"matrix("));
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
    assert_eq!(
        image::load_from_memory(&png).unwrap().to_rgba16().as_raw(),
        &flatten(&original.composite_tree(), 0).to_srgba16()
    );
    assert_eq!(
        editor.doc, original,
        "Save and exports cannot change authored sources"
    );
    let mut reopened = Editor::new(doc.clone(), None);
    let before = reopened.doc.clone();
    background::remove_image(&mut reopened).unwrap();
    assert_eq!(background::color(&reopened.doc), [80, 40, 20, 255]);
    assert!(reopened.undo());
    assert_eq!(reopened.doc, before);
}

#[test]
fn legacy_shape_only_background_roundtrip_preserves_normal_and_multiply_pixels() {
    use emulsion_core::design_background as background;
    use emulsion_raster::BlendMode;
    for (mode, opaque, translucent) in [
        (
            BlendMode::Normal,
            [0, 0, 65535, 65535],
            [32767, 0, 32768, 65535],
        ),
        (BlendMode::Multiply, [0, 0, 0, 65535], [32767, 0, 0, 65535]),
        (
            BlendMode::Screen,
            [65535, 0, 65535, 65535],
            [65535, 0, 32768, 65535],
        ),
    ] {
        let mut doc = Document::new(12, 8);
        doc.source_depth = 16;
        let mut editor = Editor::new(doc, None);
        background::set_color(&mut editor, [255, 0, 0, 255]).unwrap();
        let image = background::replace_image(
            &mut editor,
            Arc::new(Raster::from_fn(12, 8, [0; 4], |x, _| match x {
                0..4 => [0, 0, 65535, 65535],
                4..8 => [0, 0, 32768, 32768],
                _ => [0; 4],
            })),
        )
        .unwrap();
        editor.doc.node_mut(image).unwrap().blend = mode;
        let role = background::parts(&editor.doc).unwrap().image.unwrap();
        let boundary = editor.doc.node(role.boundary).unwrap().clone();
        // This is the existing native encoding, not migrated source data.
        assert_eq!(boundary.opacity, 0.);
        assert_eq!(boundary.blending, Default::default());
        let original = editor.doc.clone();
        let project = ProjectEditor::new_project(ProjectKind::Design, original.clone())
            .unwrap()
            .snapshot()
            .unwrap();
        let mut archive = Cursor::new(Vec::new());
        crate::project::write_to(&project, &mut archive).unwrap();
        let restored = crate::project::read_from(Cursor::new(archive.into_inner())).unwrap();
        for doc in [&original, &restored.pages[0].doc] {
            assert_eq!(doc.node(role.boundary), Some(&boundary));
            assert_eq!(doc.node(image).unwrap().blend, mode);
            let flat = flatten(&doc.composite_tree(), 0);
            assert_eq!(flat.get(2, 4), opaque, "{mode:?} opaque source");
            assert_eq!(flat.get(6, 4), translucent, "{mode:?} translucent source");
            assert_eq!(flat.get(10, 4), [65535, 0, 0, 65535]);
            let (svg, fallback) = crate::project_export::svg(doc).unwrap();
            assert!(fallback, "The shape-only boundary retains faithful export");
            let encoded = std::str::from_utf8(&svg)
                .unwrap()
                .split("data:image/png;base64,")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            let png = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap();
            assert_eq!(
                image::load_from_memory(&png).unwrap().to_rgba16().as_raw(),
                &flat.to_srgba16()
            );
        }
        assert_eq!(
            editor.doc, original,
            "Save and export retain authored state"
        );
    }
}

#[test]
fn invisible_page_boundary_matches_native_raster_svg_and_pdf_and_keeps_fallback() {
    use emulsion_core::design_background as background;
    use emulsion_raster::BlendMode;
    let mut editor = Editor::new(Document::new(24, 8), None);
    background::set_color(&mut editor, [255, 0, 0, 255]).unwrap();
    let source = Arc::new(Raster::from_fn(24, 8, [0; 4], |x, _| {
        if x < 8 {
            [0, 65535, 0, 65535]
        } else if x < 16 {
            [0, 0, 32768, 32768]
        } else {
            [0; 4]
        }
    }));
    background::replace_image(&mut editor, source).unwrap();
    let native = flatten(&editor.doc.composite_tree(), 0).to_srgba8();
    let (svg, fallback) = crate::project_export::svg(&editor.doc).unwrap();
    assert!(
        fallback,
        "Linear-light blue over red needs compositor export"
    );
    let encoded = std::str::from_utf8(&svg)
        .unwrap()
        .split("data:image/png;base64,")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let png = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    assert_eq!(
        image::load_from_memory(&png).unwrap().to_rgba16().as_raw(),
        &flatten(&editor.doc.composite_tree(), 0).to_srgba16(),
        "Decoded fallback retains the exact canonical blue-over-red composite"
    );
    let tree = resvg::usvg::Tree::from_data(&svg, &Default::default()).unwrap();
    let mut bitmap = resvg::tiny_skia::Pixmap::new(24, 8).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut bitmap.as_mut(),
    );
    // The exact RGBA16 fallback equality above is authoritative. Resvg's
    // 16-bit PNG to 8-bit conversion rounds independently of the native
    // linear-to-sRGB lookup, so permit at most one code value per channel.
    assert_eq!(bitmap.data().len(), native.len());
    for (channel, (actual, expected)) in bitmap.data().iter().zip(&native).enumerate() {
        assert!(
            actual.abs_diff(*expected) <= 1,
            "SVG channel {channel}: {actual}, expected {expected} within one 8-bit code value"
        );
    }
    let project = ProjectEditor::new_project(ProjectKind::Design, editor.doc.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let pdf = directory.path().join("page-background.pdf");
    let report = crate::project_export::write(
        &project,
        &[project.pages[0].meta.id],
        crate::project_export::Format::Pdf,
        false,
        &pdf,
    )
    .unwrap();
    assert_eq!(report.rasterized_pages.len(), 1);
    // Poppler is optional on development machines. When installed, validate the
    // actual PDF image independently of the SVG-to-PDF serialization path.
    if std::process::Command::new("pdftoppm")
        .arg("-v")
        .output()
        .is_ok()
    {
        let prefix = directory.path().join("rendered-background");
        let output = std::process::Command::new("pdftoppm")
            .args([
                "-scale-to-x",
                "24",
                "-scale-to-y",
                "8",
                "-aa",
                "no",
                "-aaVector",
                "no",
                "-singlefile",
                "-png",
            ])
            .arg(&pdf)
            .arg(&prefix)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rendered = image::open(prefix.with_extension("png"))
            .unwrap()
            .to_rgba8();
        assert_eq!(rendered.dimensions(), (24, 8));
        for (x, expected) in [
            (3, [0, 255, 0, 255]),
            (12, [187, 0, 188, 255]),
            (21, [255, 0, 0, 255]),
        ] {
            let actual = rendered.get_pixel(x, 4).0;
            assert!(
                actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
                "PDF color at x={x}: {actual:?}, expected {expected:?}"
            );
            assert_eq!(
                &native[((4 * 24 + x) * 4) as usize..((4 * 24 + x + 1) * 4) as usize],
                &expected
            );
        }
        eprintln!("Page background PDF independently rendered and checked with Poppler");
    } else {
        eprintln!(
            "SKIP independent page-background PDF pixel check: pdftoppm is not installed; native/SVG decoded pixels and PDF export were checked"
        );
    }
    let role = background::parts(&editor.doc).unwrap().image.unwrap();
    editor
        .execute(Command::SetBlend {
            id: role.image,
            blend: BlendMode::Multiply,
        })
        .unwrap();
    assert!(
        crate::project_export::svg(&editor.doc).unwrap().1,
        "Non-default image blend retains rendered fallback"
    );
    editor.undo();
    let mut options = editor.doc.node(role.boundary).unwrap().blending;
    options.blend_clipped_layers_as_group = false;
    editor
        .execute(Command::SetBlendingOptions {
            id: role.boundary,
            options,
        })
        .unwrap();
    assert!(
        crate::project_export::svg(&editor.doc).unwrap().1,
        "Base-opacity-dependent clips retain fallback"
    );
}

#[test]
fn hiding_background_boundary_hides_clipped_photo_in_both_native_and_svg() {
    use emulsion_core::design_background as background;
    let mut editor = Editor::new(Document::new(16, 8), None);
    background::set_color(&mut editor, [255, 0, 0, 255]).unwrap();
    background::replace_image(
        &mut editor,
        Arc::new(Raster::solid(16, 8, [0., 1., 0., 1.])),
    )
    .unwrap();
    let frame = background::parts(&editor.doc).unwrap().image.unwrap();
    editor
        .execute(Command::SetVisible {
            id: frame.boundary,
            visible: false,
        })
        .unwrap();
    let native = flatten(&editor.doc.composite_tree(), 0).to_srgba8();
    assert!(
        native
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [255, 0, 0, 255])
    );
    let (svg, fallback) = crate::project_export::svg(&editor.doc).unwrap();
    assert!(!fallback);
    assert!(!String::from_utf8_lossy(&svg).contains("<image "));
    let tree = resvg::usvg::Tree::from_data(&svg, &Default::default()).unwrap();
    let mut bitmap = resvg::tiny_skia::Pixmap::new(16, 8).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut bitmap.as_mut(),
    );
    assert_eq!(bitmap.data(), &native);
    assert!(editor.undo());
    assert_eq!(
        flatten(&editor.doc.composite_tree(), 0).get(8, 4),
        [0, 65535, 0, 65535]
    );
}
