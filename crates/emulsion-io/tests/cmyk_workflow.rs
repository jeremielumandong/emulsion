use emulsion_core::{Command, NodeKind};
use emulsion_raster::composite::flatten;

// An independent, minimal TIFF fixture: chunky, uncompressed CMYK ink values.
fn cmyk_tiff() -> Vec<u8> {
    let tags: [(u16, u16, u32, u32); 10] = [
        (256, 4, 1, 2),   // width
        (257, 4, 1, 1),   // height
        (258, 3, 4, 134), // bits per sample
        (259, 3, 1, 1),   // no compression
        (262, 3, 1, 5),   // separated inks
        (273, 4, 1, 142), // strip offset
        (277, 3, 1, 4),   // samples per pixel
        (278, 4, 1, 1),   // rows per strip
        (279, 4, 1, 8),   // strip byte count
        (284, 3, 1, 1),   // chunky
    ];
    let mut bytes = b"II\x2a\0\x08\0\0\0".to_vec();
    bytes.extend_from_slice(&(tags.len() as u16).to_le_bytes());
    for (tag, kind, count, value) in tags {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for _ in 0..4 {
        bytes.extend_from_slice(&8u16.to_le_bytes());
    }
    bytes.extend_from_slice(&[255, 0, 0, 0, 0, 0, 0, 255]); // cyan, black
    bytes
}

#[test]
fn cmyk_image_opens_edits_and_survives_native_save() {
    let dir = std::env::temp_dir().join(format!("emulsion-cmyk-workflow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("inks.tif");
    let project = dir.join("edited.ora");
    let bytes = cmyk_tiff();
    std::fs::write(&source, &bytes).unwrap();
    let mut doc = emulsion_io::open(&source).unwrap();
    let expected = [0, 255, 255, 255, 0, 0, 0, 255];
    assert_eq!(flatten(&doc.composite_tree(), 0).to_srgba8(), expected);
    let memory = emulsion_io::import::import_bytes("CMYK", &bytes).unwrap();
    assert_eq!(flatten(&memory.composite_tree(), 0).to_srgba8(), expected);
    let id = doc.nodes[0].id;
    assert!(matches!(doc.nodes[0].kind, NodeKind::Raster { .. }));
    Command::SetOpacity { id, opacity: 0.5 }
        .apply(&mut doc)
        .unwrap();
    emulsion_io::save(&doc, &project).unwrap();
    let reopened = emulsion_io::open(&project).unwrap();
    assert_eq!(reopened.nodes[0].opacity, 0.5);
    assert_eq!(
        flatten(&reopened.composite_tree(), 0).to_srgba8(),
        flatten(&doc.composite_tree(), 0).to_srgba8(),
    );
    assert_eq!(
        std::fs::read(&source).unwrap(),
        bytes,
        "source stays intact"
    );
    std::fs::remove_file(source).unwrap();
    std::fs::remove_file(project).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn planar_cmyk_tiff_interleaves_all_four_ink_planes() {
    let mut bytes = cmyk_tiff();
    // Four strips (one per ink plane), each containing two pixels.
    // Replace strip offsets/counts with pointers to arrays after the IFD.
    let strip_offsets_entry = 10 + 5 * 12;
    bytes[strip_offsets_entry + 4..strip_offsets_entry + 8].copy_from_slice(&4u32.to_le_bytes());
    bytes[strip_offsets_entry + 8..strip_offsets_entry + 12].copy_from_slice(&142u32.to_le_bytes());
    let strip_counts_entry = 10 + 8 * 12;
    bytes[strip_counts_entry + 4..strip_counts_entry + 8].copy_from_slice(&4u32.to_le_bytes());
    bytes[strip_counts_entry + 8..strip_counts_entry + 12].copy_from_slice(&158u32.to_le_bytes());
    let planar_entry = 10 + 9 * 12;
    bytes[planar_entry + 8..planar_entry + 12].copy_from_slice(&2u32.to_le_bytes());
    bytes.truncate(142);
    for offset in [174u32, 176, 178, 180] {
        bytes.extend_from_slice(&offset.to_le_bytes());
    }
    for _ in 0..4 {
        bytes.extend_from_slice(&2u32.to_le_bytes());
    }
    bytes.extend_from_slice(&[255, 0, 0, 255, 0, 0, 0, 127]);
    let doc = emulsion_io::import::import_bytes("Planar CMYK", &bytes).unwrap();
    assert_eq!(
        flatten(&doc.composite_tree(), 0).to_srgba8(),
        [0, 255, 255, 255, 128, 0, 128, 255],
    );
}
