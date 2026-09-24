//! Local camera fixture; never substitutes the embedded camera preview.
use emulsion_io::raw::{DevelopParams, RawSource};

#[test]
#[ignore = "requires EMULSION_NIKON_HE_FILE pointing to a real Nikon HE/HE★ NEF"]
fn nikon_he_decodes_sensor_and_develops_edits() {
    let path = std::path::PathBuf::from(
        std::env::var_os("EMULSION_NIKON_HE_FILE").expect("set EMULSION_NIKON_HE_FILE"),
    );
    let source = RawSource::load(&path).expect("decode Nikon HE sensor data");
    assert!(source.metadata.compression.contains("HE/HE★"));
    assert!(
        source
            .metadata
            .warnings
            .iter()
            .any(|w| w.contains("Experimental Nikon"))
    );
    assert_eq!(source.metadata.bits_per_sample, 14);
    assert!(source.info.width > 4000 && source.info.height > 2000);
    let baseline = source.develop_with(&DevelopParams::default()).unwrap();
    let edited = source
        .develop_with(&DevelopParams {
            exposure: 1.0,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        (baseline.width(), baseline.height()),
        (edited.width(), edited.height())
    );
    let before = baseline.to_srgba8();
    let after = edited.to_srgba8();
    assert_ne!(before, after, "exposure must develop sensor data");
    let luma_sum = |pixels: &[u8]| {
        pixels
            .chunks_exact(4)
            .map(|p| u64::from(p[0]) + u64::from(p[1]) + u64::from(p[2]))
            .sum::<u64>()
    };
    assert!(luma_sum(&after) > luma_sum(&before));
}
