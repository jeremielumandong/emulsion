mod common {
    pub mod raw_fixture;
}
use emulsion_io::{
    photo_develop::PhotoSource,
    photo_hdr::{self, Options},
    raw::DevelopParams,
};
use std::sync::atomic::AtomicBool;

#[test]
fn camera_orientation_hdr_and_manual_rotation_roundtrip_without_original_writes() {
    let dir = tempfile::tempdir().unwrap();
    let paths = [dir.path().join("a.dng"), dir.path().join("b.dng")];
    for path in &paths {
        common::raw_fixture::write_dng_variant(path, false, 6);
    }
    let before = paths
        .iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect::<Vec<_>>();
    let cancel = AtomicBool::new(false);
    let source = PhotoSource::load(&paths[0]).unwrap();
    let base = source.develop_with(&Default::default()).unwrap();
    let rotated = source
        .develop_with(&DevelopParams {
            rotation: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        (rotated.width(), rotated.height()),
        (base.height(), base.width())
    );
    for y in 0..base.height() {
        for x in 0..base.width() {
            assert_eq!(rotated.get(base.height() - 1 - y, x), base.get(x, y));
        }
    }
    drop(source);
    let merged = photo_hdr::merge(
        &paths,
        &Options {
            align: false,
            exposure_ev: Some(vec![-1., 1.]),
            ..Default::default()
        },
        false,
        &cancel,
    )
    .unwrap();
    assert_eq!(
        (merged.image.width, merged.image.height),
        (base.width(), base.height())
    );
    let output = dir.path().join("HDR.tif");
    merged.save(&output, &cancel).unwrap();
    let hdr = PhotoSource::load(&output).unwrap();
    assert_eq!(hdr.metadata.bits_per_sample, 32);
    let image = hdr
        .develop_with(&DevelopParams {
            rotation: 1,
            exposure: -1.,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        (image.width(), image.height()),
        (base.height(), base.width())
    );
    for (path, bytes) in paths.iter().zip(before) {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn profile_browser_previews_use_actual_profile_and_persist_favorites() {
    use rawler::formats::tiff::{DirectoryWriter, TiffWriter, Value};
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join("photo.dng");
    common::raw_fixture::write_dng(&raw);
    let source = PhotoSource::load(&raw).unwrap();
    let path = dir.path().join("test.dcp");
    let writer = TiffWriter::new(std::fs::File::create(&path).unwrap()).unwrap();
    let mut tags = DirectoryWriter::new();
    tags.add_tag(50936u16, "Emulsion generated profile browser test");
    tags.add_tag(
        50708u16,
        format!("{} {}", source.metadata.make, source.metadata.model).as_str(),
    );
    tags.add_tag(50778u16, 21u16);
    tags.add_tag(
        50721u16,
        Value::Float(vec![1., 0., 0., 0., 1., 0., 0., 0., 1.]),
    );
    tags.add_tag(50940u16, Value::Float(vec![0., 0., 0.5, 0.7, 1., 1.]));
    writer.build(tags).unwrap();
    let profile = emulsion_io::camera_profiles::install(&path).unwrap();
    let cancel = AtomicBool::new(false);
    let params = DevelopParams::default();
    let base = emulsion_io::photo_profiles::preview(&source, &params, None, &cancel).unwrap();
    let preview =
        emulsion_io::photo_profiles::preview(&source, &params, Some(profile.digest), &cancel)
            .unwrap();
    let actual = source
        .develop_preview(
            &DevelopParams {
                camera_profile: Some(profile.digest),
                ..params
            },
            &cancel,
        )
        .unwrap();
    assert_eq!(preview.to_pixels(), actual.to_pixels());
    assert_ne!(preview.to_pixels(), base.to_pixels());
    emulsion_io::photo_profiles::set_favorite(profile.digest, true).unwrap();
    assert!(emulsion_io::photo_profiles::favorites().contains(&profile.digest));
    emulsion_io::photo_profiles::set_favorite(profile.digest, false).unwrap();
    assert!(!emulsion_io::photo_profiles::favorites().contains(&profile.digest));
}
