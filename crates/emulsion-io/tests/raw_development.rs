#[path = "common/raw_fixture.rs"]
mod raw_fixture;
use emulsion_io::raw::{DevelopParams, RawSource};

#[test]
fn actual_dng_decode_bayer_xtrans_and_source_verification() {
    let directory =
        std::env::temp_dir().join(format!("emulsion-raw-development-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    for xtrans in [false, true] {
        let path = directory.join(if xtrans { "xtrans.dng" } else { "bayer.dng" });
        if xtrans {
            raw_fixture::write_dng_variant(&path, true, 6);
        } else {
            raw_fixture::write_dng(&path);
        }
        let source = RawSource::load(&path).unwrap();
        assert_eq!(source.metadata.bits_per_sample, 16);
        assert_eq!(source.metadata.compression, "uncompressed");
        let raster = source.develop_with(&DevelopParams::default()).unwrap();
        assert_eq!(
            (raster.width(), raster.height()),
            if xtrans { (24, 36) } else { (36, 24) }
        );
        assert!(raster.to_pixels().iter().any(|p| p[0] > 1000));
        RawSource::load_verified(&path, &source.source_sha256).unwrap();
        std::fs::write(&path, b"replaced original").unwrap();
        assert!(RawSource::load_verified(&path, &source.source_sha256).is_err());
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(directory).unwrap();
}

/// Regression for Nikon D90 files whose decoder does not expose the sensor IFD.
#[test]
#[ignore = "requires local Nikon RAW samples"]
fn nikon_sources_share_the_memory_budget_using_sensor_dimensions() {
    let folder = std::path::PathBuf::from(
        std::env::var_os("EMULSION_LIBRARY_RAW_SAMPLES").expect("sample folder"),
    );
    let mut paths = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| emulsion_io::raw::is_raw(p))
        .collect::<Vec<_>>();
    paths.sort();
    assert!(paths.len() >= 2);
    let metadata = emulsion_io::raw_probe::metadata(&paths[0]).unwrap();
    assert!(metadata.width > 0 && metadata.height > 0);
    let first = RawSource::load(&paths[0]).unwrap();
    let second =
        RawSource::load(&paths[1]).expect("a cached RAW must not consume the entire decode budget");
    assert!(
        u64::from(first.metadata.width) * u64::from(first.metadata.height)
            + u64::from(second.metadata.width) * u64::from(second.metadata.height)
            < emulsion_io::raw::MAX_RAW_PIXELS
    );
    second.develop_with(&Default::default()).unwrap();
    std::hint::black_box(first);
}
