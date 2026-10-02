//! Regression coverage for the page organizer's snapshot export contract.
use super::*;
use emulsion_core::{
    Command, Node,
    command::Slot,
    graph::Graph,
    project::{PageMeta, ProjectKind, ProjectPage},
};
use std::sync::Arc;

fn project() -> Project {
    let mut pages = Vec::new();
    for (id, width, height, resolution, color) in [
        (1, 20, 12, 72., [255, 0, 0, 255]),
        (2, 24, 16, 96., [0, 255, 0, 255]),
        (3, 30, 18, 144., [0, 0, 255, 255]),
        (4, 26, 20, 72., [255, 0, 255, 255]),
    ] {
        let mut doc = Document::new(width, height);
        doc.resolution = resolution;
        Command::AddNode {
            node: Box::new(Node::new(0, "Background", NodeKind::Fill { rgba: color })),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Position marker",
                Arc::new(emulsion_raster::vector_geometry::rectangle(2., 3., 4., 4.)),
                emulsion_raster::vector::PathStyle {
                    fill: Some([255, 255, 255, 255]),
                    stroke: None,
                    ..Default::default()
                },
                width,
                height,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        pages.push(ProjectPage {
            meta: PageMeta {
                id,
                name: format!("Artwork {id}"),
                bleed_mm: 2.54,
            },
            graph: Graph::new(doc.clone(), "Original artwork"),
            doc,
        });
    }
    // Layout order is deliberately different from stable ID order.
    pages.rotate_left(2);
    let project = Project {
        kind: ProjectKind::Design,
        pages,
        active: 4,
        next_page_id: 5,
    };
    project.validate().unwrap();
    project
}

fn assert_unchanged(actual: &Project, original: &Project) {
    assert_eq!(actual.kind, original.kind);
    assert_eq!(actual.active, original.active);
    assert_eq!(actual.next_page_id, original.next_page_id);
    assert_eq!(actual.pages.len(), original.pages.len());
    for (actual, original) in actual.pages.iter().zip(&original.pages) {
        assert_eq!(actual.meta, original.meta);
        assert_eq!(actual.doc, original.doc);
        assert_eq!(
            format!("{:?}", actual.graph),
            format!("{:?}", original.graph)
        );
    }
}

#[test]
fn selected_png_pages_keep_document_order_artwork_dimensions_and_optional_bleed() {
    let project = project();
    let original = project.clone();
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("selected.zip");
    for include_bleed in [false, true] {
        // Reverse selection order, skip pages between/after the chosen pages,
        // and omit the active page. Only current document order should matter.
        let report = write(&project, &[1, 3], Format::Png, include_bleed, &destination).unwrap();
        assert_eq!(report.pages, 2);
        assert!(report.rasterized_pages.is_empty());
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&destination).unwrap()).unwrap();
        assert_eq!(zip.len(), 2);
        for (index, name, width, height, bleed, color) in [
            (0, "page-001-3.png", 30, 18, 14, [0, 0, 255, 255]),
            (1, "page-003-1.png", 20, 12, 7, [255, 0, 0, 255]),
        ] {
            assert_eq!(zip.by_index(index).unwrap().name(), name);
            let bytes = crate::ora::read_entry(&mut zip, name, 1 << 20).unwrap();
            let image = image::load_from_memory(&bytes).unwrap().to_rgba8();
            let bleed = if include_bleed { bleed } else { 0 };
            assert_eq!(image.dimensions(), (width + 2 * bleed, height + 2 * bleed));
            assert_eq!(*image.get_pixel(bleed, bleed), image::Rgba(color));
            assert_eq!(
                *image.get_pixel(bleed + 3, bleed + 4),
                image::Rgba([255; 4])
            );
            // Bleed must expose artwork, not add a blank border.
            assert_eq!(*image.get_pixel(0, 0), image::Rgba(color));
        }
        assert_unchanged(&project, &original);
    }
}

fn pdf_array<'a>(object: &'a str, name: &str) -> Vec<&'a str> {
    object
        .split_once(&format!("/{name} ["))
        .unwrap()
        .1
        .split_once(']')
        .unwrap()
        .0
        .split_whitespace()
        .collect()
}

fn pdf_object<'a>(pdf: &'a str, id: &str) -> &'a str {
    pdf.split_once(&format!("\n{id} 0 obj\n"))
        .unwrap()
        .1
        .split_once("\nendobj")
        .unwrap()
        .0
}

fn pdf_artwork_stream(pdf: &[u8], id: &str) -> String {
    use std::io::Read as _;
    let header = format!("\n{id} 0 obj\n");
    let offset = pdf
        .windows(header.len())
        .position(|bytes| bytes == header.as_bytes())
        .unwrap();
    let object = &pdf[offset + header.len()..];
    let stream = b"\nstream\n";
    let offset = object
        .windows(stream.len())
        .position(|bytes| bytes == stream)
        .unwrap();
    let dictionary = std::str::from_utf8(&object[..offset]).unwrap();
    assert!(dictionary.contains("/Filter /FlateDecode"));
    let length = dictionary
        .split_once("/Length ")
        .unwrap()
        .1
        .split_whitespace()
        .next()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let encoded = &object[offset + stream.len()..][..length];
    let mut decoded = String::new();
    flate2::read::ZlibDecoder::new(encoded)
        .read_to_string(&mut decoded)
        .unwrap();
    decoded
}

#[test]
fn selected_pdf_page_tree_keeps_document_order_and_each_pages_physical_boxes() {
    let project = project();
    let original = project.clone();
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("selected.pdf");
    for include_bleed in [false, true] {
        let report = write(&project, &[1, 3], Format::Pdf, include_bleed, &destination).unwrap();
        assert_eq!(report.pages, 2);
        assert!(report.rasterized_pages.is_empty());
        let bytes = std::fs::read(&destination).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        let pdf = String::from_utf8_lossy(&bytes);
        let tree = pdf
            .split("\nendobj")
            .find(|object| object.contains("/Type /Pages\n"))
            .unwrap();
        assert!(tree.contains("/Count 2\n"));
        let kids = pdf_array(tree, "Kids");
        assert_eq!(kids.len(), 6);
        // Read actual /Kids references, rather than relying on physical object
        // placement in the file. Unique page sizes identify their intended order.
        for (reference, (width, height, color)) in kids
            .as_chunks::<3>()
            .0
            .iter()
            .zip([(15., 9., "0 0 1 scn"), (20., 12., "1 0 0 scn")])
        {
            assert_eq!(&reference[1..], &["0", "R"]);
            let page = pdf_object(&pdf, reference[0]);
            assert!(page.contains("/Type /Page\n"));
            let b = if include_bleed { 7. } else { 0. };
            for (name, expected) in [
                ("MediaBox", [0., 0., width + 2. * b, height + 2. * b]),
                ("BleedBox", [0., 0., width + 2. * b, height + 2. * b]),
                ("TrimBox", [b, b, width + b, height + b]),
            ] {
                let actual = pdf_array(page, name)
                    .iter()
                    .map(|value| value.parse::<f64>().unwrap())
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected);
            }
            let artwork_id = page
                .split_once("/Artwork ")
                .unwrap()
                .1
                .split_whitespace()
                .next()
                .unwrap();
            let artwork = pdf_artwork_stream(&bytes, artwork_id);
            assert!(artwork.contains(color), "Wrong page artwork: {artwork}");
            assert!(artwork.contains("1 1 1 scn"), "Missing position marker");
        }
        assert_unchanged(&project, &original);
    }
}

#[test]
fn single_selected_page_does_not_expand_to_all_pages() {
    let project = project();
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("single.zip");
    assert_eq!(
        write(&project, &[1], Format::Png, false, &destination)
            .unwrap()
            .pages,
        1
    );
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&destination).unwrap()).unwrap();
    assert_eq!(zip.len(), 1);
    assert_eq!(zip.by_index(0).unwrap().name(), "page-003-1.png");
    drop(zip);
    let destination = directory.path().join("single.pdf");
    assert_eq!(
        write(&project, &[1], Format::Pdf, false, &destination)
            .unwrap()
            .pages,
        1
    );
    let pdf = std::fs::read(&destination).unwrap();
    let pdf = String::from_utf8_lossy(&pdf);
    assert!(pdf.contains("/Count 1\n"));
    assert_eq!(pdf.matches("/Type /Page\n").count(), 1);
}

#[test]
fn empty_duplicate_invalid_and_stale_selections_never_create_or_replace_an_export() {
    let mut project = project();
    // A deleted selected ID must not silently disappear from a mixed selection.
    project.pages.retain(|page| page.meta.id != 2);
    project.validate().unwrap();
    let original = project.clone();
    let directory = tempfile::tempdir().unwrap();
    for format in Format::ALL {
        for selected in [
            &[][..],
            &[0][..],
            &[999][..],
            &[1, 999][..],
            &[3, 3][..],
            &[1, 2][..],
        ] {
            let absent = directory.path().join("absent");
            let existing = directory.path().join("existing");
            std::fs::write(&existing, b"previous successful export").unwrap();
            for destination in [&absent, &existing] {
                let error = write(&project, selected, format, false, destination).unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("Select existing pages to export.")
                );
            }
            assert!(!absent.exists());
            assert_eq!(
                std::fs::read(&existing).unwrap(),
                b"previous successful export"
            );
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
            assert_unchanged(&project, &original);
        }
    }
}

#[test]
fn a_later_selected_page_failure_keeps_destination_and_removes_partial_files() {
    let mut project = project();
    // The first selected page completes. Only the second page's expanded bleed
    // fails the image-size limit, after the atomic write has already started.
    let second = project
        .pages
        .iter_mut()
        .find(|page| page.meta.id == 1)
        .unwrap();
    second.doc.resolution = 4000.;
    second.meta.bleed_mm = 100.;
    project.validate().unwrap();
    let original = project.clone();
    let directory = tempfile::tempdir().unwrap();
    for format in [Format::Png, Format::Pdf] {
        let absent = directory.path().join("absent");
        let existing = directory.path().join("existing");
        std::fs::write(&existing, b"previous successful export").unwrap();
        for destination in [&absent, &existing] {
            assert!(matches!(
                write(&project, &[1, 3], format, true, destination),
                Err(IoError::TooLarge(_, _))
            ));
        }
        assert!(!absent.exists());
        assert_eq!(
            std::fs::read(&existing).unwrap(),
            b"previous successful export"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        assert_unchanged(&project, &original);
    }
}
