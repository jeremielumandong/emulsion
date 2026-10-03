use super::*;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/ocio")
        .join(name)
}

#[track_caller]
fn close(got: [f64; 3], want: [f64; 3], tol: f64) {
    for c in 0..3 {
        let scale = want[c].abs().max(1.);
        assert!(
            (got[c] - want[c]).abs() <= tol * scale,
            "channel {c}: got {got:?}, want {want:?} (tolerance {tol})"
        );
    }
}

fn run(t: &str, rgb: [f64; 3]) -> [f64; 3] {
    let value = transform::parse_yaml(t).unwrap();
    let t = transform::parse(&value).unwrap();
    Config::builtin()
        .transform_processor(&t)
        .unwrap()
        .apply_f64(rgb)
}

fn builtin(style: &str, rgb: [f64; 3]) -> [f64; 3] {
    run(&format!("!<BuiltinTransform> {{style: \"{style}\"}}"), rgb)
}

// ── Matrices ─────────────────────────────────────────────────────────────

#[test]
fn aces_primaries_give_the_published_matrices() {
    // ACES TB-2014-004 (AP0 → XYZ) and S-2014-004 (AP1 → XYZ, AP0 ↔ AP1).
    let ap0_xyz = [
        [0.9525523959, 0.0, 0.0000936786],
        [0.3439664498, 0.7281660966, -0.0721325464],
        [0.0, 0.0, 1.0088251844],
    ];
    let ap1_xyz = [
        [0.6624541811, 0.1340042065, 0.1561876870],
        [0.2722287168, 0.6740817658, 0.0536895174],
        [-0.0055746495, 0.0040607335, 1.0103391003],
    ];
    let ap0_ap1 = [
        [1.4514393161, -0.2365107469, -0.2149285693],
        [-0.0765537734, 1.1762296998, -0.0996759264],
        [0.0083161484, -0.0060324498, 0.9977163014],
    ];
    let ap1_ap0 = [
        [0.6954522414, 0.1406786965, 0.1638690622],
        [0.0447945634, 0.8596711185, 0.0955343182],
        [-0.0055258826, 0.0040252103, 1.0015006723],
    ];
    let check = |m: moxcms::Matrix3d, want: [[f64; 3]; 3]| {
        for (r, row) in want.iter().enumerate() {
            for (c, w) in row.iter().enumerate() {
                assert!(
                    (m.v[r][c] - w).abs() < 2e-7,
                    "[{r}][{c}] {} vs {w}",
                    m.v[r][c]
                );
            }
        }
    };
    check(rgb_to_xyz(&AP0), ap0_xyz);
    check(rgb_to_xyz(&AP1), ap1_xyz);
    check(conversion(&AP0, &AP1, false), ap0_ap1);
    check(conversion(&AP1, &AP0, false), ap1_ap0);
    // sRGB / Rec.709 from the D65 chromaticities.
    check(
        rgb_to_xyz(&REC709),
        [
            [0.4123908, 0.3575843, 0.1804808],
            [0.2126390, 0.7151687, 0.0721923],
            [0.0193308, 0.1191948, 0.9505322],
        ],
    );
}

/// Reference values from OpenColorIO's own BuiltinTransform unit tests
/// (BSD-3-Clause), input (0.5, 0.4, 0.3) unless stated.
#[test]
fn builtin_transforms_match_opencolorio_reference_values() {
    let i = [0.5, 0.4, 0.3];
    let cases: &[(&str, [f64; 3], f64)] = &[
        ("IDENTITY", [0.5, 0.4, 0.3], 1e-9),
        (
            "UTILITY - ACES-AP0_to_CIE-XYZ-D65_BFD",
            [0.472347603390, 0.440425934827, 0.326581044758],
            1e-6,
        ),
        (
            "UTILITY - ACES-AP1_to_CIE-XYZ-D65_BFD",
            [0.428407900093, 0.420968434905, 0.325777868096],
            1e-6,
        ),
        (
            "UTILITY - ACES-AP1_to_LINEAR-REC709_BFD",
            [0.578830986466, 0.388029190156, 0.282302431033],
            1e-6,
        ),
        (
            "CURVE - ACEScct-LOG_to_LINEAR",
            [0.514056913328, 0.152618314084, 0.045310838527],
            1e-6,
        ),
        (
            "ACEScct_to_ACES2065-1",
            [0.386397222658, 0.158557251811, 0.043152537925],
            1e-6,
        ),
        (
            "ACEScc_to_ACES2065-1",
            [0.386397222658, 0.158557251811, 0.043152537925],
            1e-6,
        ),
        (
            "ACEScg_to_ACES2065-1",
            [0.453158317919, 0.394926024520, 0.299297344519],
            1e-6,
        ),
        (
            "ACES-LMT - BLUE_LIGHT_ARTIFACT_FIX",
            [0.48625676579, 0.38454173877, 0.30002108779],
            1e-6,
        ),
        (
            "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-DCI-BFD",
            [0.908856342287, 0.627840575107, 0.608053675805],
            1e-6,
        ),
        (
            "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-D60-BFD",
            [0.892433142142, 0.627011653770, 0.608093643982],
            1e-6,
        ),
        (
            "DISPLAY - CIE-XYZ-D65_to_DisplayP3",
            [0.882580907776, 0.581526360743, 0.5606367050000],
            1e-6,
        ),
        // The ACES 1.0 output transforms: OCIO fits the CTL tone scale with
        // B-splines, this crate evaluates the CTL splines themselves.
        (
            "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-CINEMA_1.0",
            [0.33629957, 0.31832799, 0.22867827],
            2e-4,
        ),
        (
            "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-VIDEO_1.0",
            [0.34128153, 0.32533440, 0.24217427],
            2e-4,
        ),
        (
            "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-CINEMA-REC709lim_1.1",
            [0.33629954, 0.31832793, 0.22867827],
            2e-4,
        ),
        (
            "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-VIDEO-REC709lim_1.1",
            [0.34128147, 0.32533434, 0.24217427],
            2e-4,
        ),
        (
            "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-VIDEO-P3lim_1.1",
            [0.34128150, 0.32533440, 0.24217424],
            2e-4,
        ),
    ];
    for (style, want, tol) in cases {
        let got = builtin(style, i);
        assert!(
            (0..3).all(|c| (got[c] - want[c]).abs() <= *tol),
            "{style}: got {got:?}, want {want:?}"
        );
    }
    close(
        builtin(
            "ACES-LMT - ACES 1.3 Reference Gamut Compression",
            [0.5, 0.4, -0.3],
        ),
        [0.54812347889, 0.42805567384, -0.00588858686],
        1e-6,
    );
    // Display encodings, including negative and over-range input.
    let display: &[(&str, [f64; 6])] = &[
        (
            "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.709",
            [
                0.937245093108,
                0.586817090358,
                0.573498106368,
                0.,
                0.505174310421,
                1.118456082347,
            ],
        ),
        (
            "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.709 - MIRROR NEGS",
            [
                0.937245093108,
                0.586817090358,
                0.573498106368,
                -0.940082660458,
                0.505174310421,
                1.118456082347,
            ],
        ),
        (
            "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.2020",
            [
                0.830338272693,
                0.620393283803,
                0.583385370254,
                0.,
                0.432629991358,
                1.069355537167,
            ],
        ),
        (
            "DISPLAY - CIE-XYZ-D65_to_G2.2-REC.709",
            [
                0.931739212204,
                0.559058879141,
                0.545230761999,
                0.,
                0.474767926071,
                1.129896956592,
            ],
        ),
        (
            "DISPLAY - CIE-XYZ-D65_to_sRGB",
            [
                0.933793573229,
                0.564092030327,
                0.550040502218,
                -11.142147651136028,
                0.477958897494,
                1.124971166876,
            ],
        ),
        (
            "DISPLAY - CIE-XYZ-D65_to_sRGB - MIRROR NEGS",
            [
                0.933793573229,
                0.564092030327,
                0.550040502218,
                -0.936787206783,
                0.477958897494,
                1.124971166876,
            ],
        ),
        (
            "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-D65",
            [
                0.896805202281,
                0.627254277624,
                0.608228132100,
                0.,
                0.493163009212,
                1.069368427937,
            ],
        ),
    ];
    for (style, want) in display {
        close(builtin(style, i), [want[0], want[1], want[2]], 1e-6);
        close(
            builtin(style, [-0.05, 0.05, 1.25]),
            [want[3], want[4], want[5]],
            1e-6,
        );
    }
    type Case = (&'static str, [f64; 3], [f64; 3], [f64; 3], [f64; 3]);
    let pq: &[Case] = &[
        (
            "CURVE - ST-2084_to_LINEAR",
            [0.5, 0.4, 0.3],
            [0.922457089941, 0.324479178538, 0.100382263105],
            [-0.1, -0.3, 1.01],
            [-0.0032456566, -0.10038226, 110.045776],
        ),
        (
            "CURVE - LINEAR_to_ST-2084",
            [0.5, 0.4, 0.3],
            [0.440281573420, 0.419284117712, 0.392876186489],
            [-0.1, 101.0, 0.2],
            [-0.299699098, 1.00104129, 0.357012421],
        ),
        (
            "DISPLAY - CIE-XYZ-D65_to_REC.2100-PQ",
            [0.5, 0.4, 0.3],
            [0.464008302136, 0.398157119110, 0.384828370950],
            [-0.1, 1.01, 0.2],
            [-0.454744577, 0.562376201, 0.328883916],
        ),
        (
            "DISPLAY - CIE-XYZ-D65_to_ST2084-P3-D65",
            [0.5, 0.4, 0.3],
            [0.479939091128, 0.392091860770, 0.384886051856],
            [-0.1, 1.01, 0.2],
            [-0.532302439, 0.572011411, 0.307887018],
        ),
    ];
    for (style, a, wa, b, wb) in pq {
        close(builtin(style, *a), *wa, 4e-5);
        close(builtin(style, *b), *wb, 4e-5);
    }
}

#[test]
fn aces_output_transforms_invert() {
    // The glow and red modifier inverses estimate their weights from the
    // output, as OCIO's do, so the output transform round trip is close
    // but not exact. ACEScc clamps negative ACES2065-1, so its codes stay
    // where AP0 is positive.
    let scene = [
        [0.18, 0.18, 0.18],
        [0.5, 0.4, 0.3],
        [0.05, 0.2, 0.1],
        [1.2, 1., 0.5],
    ];
    let codes = [[0.41, 0.41, 0.41], [0.5, 0.4, 0.3], [0.3, 0.45, 0.35]];
    for (style, tol, inputs) in [
        (
            "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-VIDEO_1.0",
            2e-2,
            &scene[..],
        ),
        ("ACEScct_to_ACES2065-1", 1e-9, &codes[..]),
        ("ACEScc_to_ACES2065-1", 1e-9, &codes[..]),
    ] {
        let fwd = format!("!<BuiltinTransform> {{style: \"{style}\"}}");
        let inv = format!("!<BuiltinTransform> {{style: \"{style}\", direction: inverse}}");
        for &rgb in inputs {
            close(run(&inv, run(&fwd, rgb)), rgb, tol);
        }
    }
}

#[test]
fn unknown_builtin_and_unsupported_transforms_are_named() {
    let value: serde_yaml::Value = transform::parse_yaml(
        "!<BuiltinTransform> {style: ARRI_ALEXA-LOGC-EI800-AWG_to_ACES2065-1}",
    )
    .unwrap();
    let err = Config::builtin()
        .transform_processor(&transform::parse(&value).unwrap())
        .unwrap_err();
    assert!(
        matches!(&err, Error::Unsupported(m) if m.contains("ARRI_ALEXA-LOGC-EI800-AWG_to_ACES2065-1")),
        "{err}"
    );
    let value: serde_yaml::Value =
        transform::parse_yaml("!<ExposureContrastTransform> {exposure: 1}").unwrap();
    let err = Config::builtin()
        .transform_processor(&transform::parse(&value).unwrap())
        .unwrap_err();
    assert!(
        err.to_string().contains("ExposureContrastTransform"),
        "{err}"
    );
}

// ── Each transform type, against hand-computed values ────────────────────

#[test]
fn matrix_exponent_log_cdl_range_and_group_transforms() {
    // Matrix with offset, and its inverse.
    let m = "!<MatrixTransform> {matrix: [2, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1], offset: [0.1, 0, -0.1, 0]}";
    close(run(m, [0.25, 0.5, 0.25]), [0.6, 0.5, 0.65], 1e-12);
    let mi = "!<MatrixTransform> {matrix: [2, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1], offset: [0.1, 0, -0.1, 0], direction: inverse}";
    close(run(mi, [0.6, 0.5, 0.65]), [0.25, 0.5, 0.25], 1e-12);
    // Exponent: clamp (default), mirror and pass-through negatives.
    close(
        run("!<ExponentTransform> {value: 2.2}", [0.5, -0.5, 1.]),
        [0.5f64.powf(2.2), 0., 1.],
        1e-12,
    );
    close(
        run(
            "!<ExponentTransform> {value: [2, 2, 2, 1], style: mirror}",
            [0.5, -0.5, 2.],
        ),
        [0.25, -0.25, 4.],
        1e-12,
    );
    close(
        run(
            "!<ExponentTransform> {value: [2, 2, 2, 1], style: pass_thru, direction: inverse}",
            [0.25, -0.5, 4.],
        ),
        [0.5, -0.5, 2.],
        1e-12,
    );
    // ExponentWithLinear: the sRGB curve decodes forward; the toe is linear.
    let srgb = "!<ExponentWithLinearTransform> {gamma: 2.4, offset: 0.055}";
    let toe = 1. / 12.923_210_180_787_86;
    close(
        run(srgb, [0.5, 0.02, -0.02]),
        [
            ((0.5 + 0.055) / 1.055f64).powf(2.4),
            0.02 * toe,
            -0.02 * toe,
        ],
        1e-6,
    );
    let srgb_inv = "!<ExponentWithLinearTransform> {gamma: 2.4, offset: 0.055, direction: inverse}";
    close(
        run(srgb_inv, run(srgb, [0.5, 0.02, 0.9])),
        [0.5, 0.02, 0.9],
        1e-9,
    );
    // Log (base 10), LogAffine (Cineon) and LogCamera (ACEScct).
    close(
        run("!<LogTransform> {base: 10}", [100., 1., 0.1]),
        [2., 0., -1.],
        1e-12,
    );
    let cineon = "!<LogAffineTransform> {base: 10, log_side_slope: 0.293255131964809, log_side_offset: 0.669599217986315, lin_side_slope: 0.9892, lin_side_offset: 0.0108}";
    // Linear 1.0 is Cineon white (685/1023); 0 is black (95/1023).
    close(
        run(cineon, [1., 0., 1.]),
        [685. / 1023., 95. / 1023., 685. / 1023.],
        1e-4,
    );
    let cct = "!<LogCameraTransform> {base: 2, log_side_slope: 0.0570776255707763, log_side_offset: 0.554794520547945, lin_side_break: 0.0078125}";
    // ACEScct: 0.18 → 0.4135884, and the linear toe at 0 → 0.0729055.
    close(
        run(cct, [0.18, 0., 0.0078125]),
        [0.413_588_402_7, 0.072_905_534_7, 0.155_251_141_6],
        1e-7,
    );
    // CDL: slope, offset, power then saturation (Rec.709 luma); ASC clamps.
    let cdl = "!<CDLTransform> {slope: [2, 1, 1], offset: [0, 0.1, 0], power: [1, 1, 2], sat: 0.5}";
    let pre: [f64; 3] = [0.6, 0.3, 0.25];
    let luma = 0.2126 * pre[0] + 0.7152 * pre[1] + 0.0722 * pre[2];
    close(
        run(cdl, [0.3, 0.2, 0.5]),
        pre.map(|v| luma + 0.5 * (v - luma)),
        1e-12,
    );
    close(
        run(
            "!<CDLTransform> {slope: [3, 3, 3], style: asc}",
            [0.5, -0.1, 0.2],
        ),
        [1., 0., 0.6],
        1e-12,
    );
    close(
        run(
            "!<CDLTransform> {slope: [3, 3, 3], power: [2, 2, 2]}",
            [0.5, -0.1, 0.2],
        ),
        [2.25, -0.3, 0.36],
        1e-12,
    );
    let cdl_inv = "!<CDLTransform> {slope: [2, 1, 1], offset: [0, 0.1, 0], power: [1, 1, 2], sat: 0.5, direction: inverse}";
    close(
        run(cdl_inv, run(cdl, [0.3, 0.2, 0.5])),
        [0.3, 0.2, 0.5],
        1e-9,
    );
    // Range: scale and clamp, no clamp, and one-sided.
    let range = "!<RangeTransform> {min_in_value: 0.1, max_in_value: 0.9, min_out_value: 0, max_out_value: 1}";
    close(run(range, [0.5, 0., 1.]), [0.5, 0., 1.], 1e-12);
    let free = "!<RangeTransform> {min_in_value: 0.1, max_in_value: 0.9, min_out_value: 0, max_out_value: 1, style: noClamp}";
    close(run(free, [0.5, 0., 1.]), [0.5, -0.125, 1.125], 1e-12);
    close(
        run(
            "!<RangeTransform> {min_in_value: 0, min_out_value: 0}",
            [-1., 5., 0.5],
        ),
        [0., 5., 0.5],
        1e-12,
    );
    // Group, forward and inverse (children reversed and inverted).
    let group = "!<GroupTransform> {children: [!<ExponentTransform> {value: 2}, !<MatrixTransform> {matrix: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1], offset: [0.5, 0.5, 0.5, 0]}]}";
    close(run(group, [0.5, 1., 0.]), [0.75, 1.5, 0.5], 1e-12);
    let group_inv = group.replacen("{children", "{direction: inverse, children", 1);
    close(run(&group_inv, [0.75, 1.5, 0.5]), [0.5, 1., 0.], 1e-12);
    // FixedFunction ACES_DarkToDim10 (luminance from AP1).
    let y: f64 = 0.272_228_716_780_914_54 * 0.5
        + 0.674_081_765_811_148_3 * 0.4
        + 0.053_689_517_407_937_05 * 0.3;
    let k = y.powf(0.9811 - 1.);
    close(
        run(
            "!<FixedFunctionTransform> {style: ACES_DarkToDim10}",
            [0.5, 0.4, 0.3],
        ),
        [0.5 * k, 0.4 * k, 0.3 * k],
        1e-12,
    );
}

#[test]
fn colorspace_transform_uses_the_config_and_skips_data() {
    let c = Config::builtin();
    let p = c
        .transform_processor(
            &transform::parse(
                &transform::parse_yaml("!<ColorSpaceTransform> {src: ACEScct, dst: ACEScg}")
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    close(
        p.apply_f64([0.4135884, 0.5, 0.3]),
        [0.18, 0.514_056_913_328, 0.045_310_838_527],
        1e-6,
    );
    let raw = c.processor("ACEScg", "Raw").unwrap();
    assert!(raw.is_noop());
}

// ── Configs ──────────────────────────────────────────────────────────────

#[test]
fn v1_config_with_roles_lut_views_and_looks() {
    let c = Config::from_file(&fixture("v1/config.ocio")).unwrap();
    assert_eq!(c.version, 1);
    assert_eq!(c.role("scene_linear"), Some("linear"));
    assert_eq!(c.colorspace("compositing_log").unwrap().name, "Cineon");
    assert!(c.colorspace("raw").unwrap().isdata);
    assert!(
        c.colorspace("linear")
            .unwrap()
            .description
            .starts_with("Scene-linear")
    );
    // Active displays and views pick and order what is offered.
    let displays: Vec<_> = c.active_displays().iter().map(|d| d.name.clone()).collect();
    assert_eq!(displays, ["default"]);
    let views: Vec<_> = c
        .active_views("default")
        .iter()
        .map(|v| v.name.clone())
        .collect();
    assert_eq!(views, ["sRGB", "rec709", "None", "Graded"]);
    assert_eq!(c.default_view("default").unwrap().name, "sRGB");
    // linear → sRGB view inverts the 1D LUT from the search path.
    let p = c
        .display_processor("linear", "default", "sRGB", None)
        .unwrap();
    close(
        p.apply_f64([0.18, 0.5, 1.]),
        [0.461_356_129, 0.735_356_983, 1.],
        2e-4,
    );
    // rec709: inverse of gamma 2.4.
    let p = c
        .display_processor("linear", "default", "rec709", None)
        .unwrap();
    close(
        p.apply_f64([0.18, 0.5, 1.]),
        [0.18f64.powf(1. / 2.4), 0.5f64.powf(1. / 2.4), 1.],
        1e-9,
    );
    // from_reference given as an inverse transform.
    let p = c
        .display_processor("linear", "monitor", "Gamma 2.2", None)
        .unwrap();
    close(
        p.apply_f64([0.25, 0.5, 1.]),
        [0.25f64.powf(1. / 2.2), 0.5f64.powf(1. / 2.2), 1.],
        1e-9,
    );
    // The None view and data spaces pass through.
    assert!(
        c.display_processor("linear", "default", "None", None)
            .unwrap()
            .is_noop()
    );
    // Looks run in their process space before the view; an override wins.
    let graded = c
        .display_processor("linear", "default", "Graded", None)
        .unwrap();
    let plain = c
        .display_processor("linear", "default", "sRGB", None)
        .unwrap();
    close(
        graded.apply_f64([0.18; 3]),
        plain.apply_f64([0.198, 0.18, 0.162]),
        1e-9,
    );
    let off = c
        .display_processor("linear", "default", "Graded", Some(""))
        .unwrap();
    close(off.apply_f64([0.18; 3]), plain.apply_f64([0.18; 3]), 1e-12);
    // Cineon → linear.
    let p = c.processor("Cineon", "linear").unwrap();
    let cineon = |cv: f64| (10f64.powf((cv * 1023. - 685.) / 300.) - 0.0108) / 0.9892;
    close(
        p.apply_f64([685. / 1023., 95. / 1023., 0.5]),
        [1., cineon(95. / 1023.), cineon(0.5)],
        1e-9,
    );
}

#[test]
fn v2_config_with_environment_search_paths_and_view_transforms() {
    let c = Config::from_file(&fixture("v2/config.ocio")).unwrap();
    assert_eq!(c.version, 2);
    assert_eq!(c.expand("$LUT_DIR/${SHOT}.cube"), "luts/sh010.cube");
    let names: Vec<_> = c
        .active_colorspaces()
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert!(names.contains(&"ACEScct".to_string()) && !names.contains(&"Hidden".to_string()));
    assert_eq!(c.colorspace("lin_ap1").unwrap().name, "ACEScg");
    assert_eq!(
        c.colorspace("srgb_display").unwrap().reference,
        Reference::Display
    );
    // Shared views and <USE_DISPLAY_NAME>.
    let views: Vec<_> = c
        .active_views("P3")
        .iter()
        .map(|v| v.name.clone())
        .collect();
    assert_eq!(views, ["Film", "Plain"]);
    assert_eq!(c.view_colorspace("sRGB", "Plain").unwrap(), "sRGB");
    // Plain view of ACEScg 0.18 grey is the sRGB encoding of 0.18.
    let p = c
        .display_processor("ACEScg", "sRGB", "Plain", None)
        .unwrap();
    let grey = p.apply_f64([0.18; 3]);
    close(grey, [0.18f64.powf(1. / 2.4) * 1.055 - 0.055; 3], 1e-5);
    // The Film view (ACES 1.0 SDR video) with the shot look: the look
    // changes red, and mid grey lands near its usual ~0.36 sRGB code.
    let film = c
        .display_processor("ACEScg", "sRGB", "Film", Some(""))
        .unwrap();
    let g = film.apply_f64([0.18; 3]);
    assert!(
        (0.33..0.38).contains(&g[1]) && (g[0] - g[2]).abs() < 1e-3,
        "{g:?}"
    );
    let looked = c.display_processor("ACEScg", "sRGB", "Film", None).unwrap();
    assert!(looked.apply_f64([0.18; 3])[0] > g[0]);
    // Scene → display colour space through the default view transform.
    let direct = c.processor("ACEScg", "sRGB").unwrap();
    close(direct.apply_f64([0.18; 3]), grey, 1e-9);
    // ACEScct defined with a LogCamera group matches the builtin.
    let cct = c.processor("ACEScct", "ACES2065-1").unwrap();
    close(
        cct.apply_f64([0.5, 0.4, 0.3]),
        [0.386397222658, 0.158557251811, 0.043152537925],
        1e-6,
    );
    // LUT files through the search path: .cube 3D (tetrahedral), .spi3d,
    // CLF, and a shaper .cube whose name comes from an environment default.
    close(
        c.processor("ACES2065-1", "Inverted")
            .unwrap()
            .apply_f64([0.2, 0.5, 0.9]),
        [0.8, 0.5, 0.1],
        1e-6,
    );
    close(
        c.processor("ACES2065-1", "Halved")
            .unwrap()
            .apply_f64([0.2, 0.5, 0.9]),
        [0.1, 0.25, 0.45],
        1e-6,
    );
    close(
        c.processor("ACES2065-1", "CLF ops")
            .unwrap()
            .apply_f64([0.2, 0.5, 0.8]),
        [4., 4., 2f64.powf(0.8).powi(2)],
        1e-6,
    );
    close(
        c.processor("ACES2065-1", "Shot LUT")
            .unwrap()
            .apply_f64([2., 1., 4.]),
        [
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2 / 2.,
            1.,
        ],
        1e-6,
    );
    // Unsupported transforms fail with their name; data passes through.
    let err = c.processor("ACEScg", "Graded").unwrap_err();
    assert!(err.to_string().contains("GradingPrimaryTransform"), "{err}");
    assert!(err.to_string().contains("Graded"), "{err}");
    assert!(c.processor("ACEScg", "Data").unwrap().is_noop());
    // A missing LUT names the file and the search path.
    let missing = Config::parse(
        "ocio_profile_version: 2\nsearch_path: [nowhere]\ncolorspaces:\n  - !<ColorSpace> {name: a}\n  - !<ColorSpace> {name: b, from_scene_reference: !<FileTransform> {src: gone.cube}}\n",
        Path::new("/tmp"),
    )
    .unwrap()
    .processor("a", "b")
    .unwrap_err();
    assert!(missing.to_string().contains("gone.cube"), "{missing}");
}

#[test]
fn configs_report_broken_references() {
    let err = Config::parse(
        "ocio_profile_version: 1\nroles: {default: nope}\ncolorspaces: []\n",
        Path::new(""),
    )
    .unwrap_err();
    assert!(err.to_string().contains("nope"), "{err}");
    assert!(Config::parse("ocio_profile_version: 3\n", Path::new("")).is_err());
    assert!(Config::parse("not: [valid", Path::new("")).is_err());
}

// ── LUT files ────────────────────────────────────────────────────────────

#[test]
fn lut_formats_parse_and_interpolate() {
    // .cube 1D with a domain.
    let ops = lut::parse_cube(
        "TITLE \"t\"\nLUT_1D_SIZE 3\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n0 0 0\n0.25 0.5 1\n1 1 1\n",
        "t.cube",
    )
    .unwrap();
    let p = Processor::new(ops);
    close(p.apply_f64([0.5, 1., 3.]), [0.125, 0.5, 1.], 1e-6);
    // Inverse of that 1D LUT (blue's flat top inverts to its start).
    let inv = p.inverse().unwrap();
    close(inv.apply_f64([0.125, 0.5, 1.]), [0.5, 1., 1.], 1e-6);
    // .cube 3D: linear vs tetrahedral differ on a nonlinear lattice.
    let mut text = String::from("LUT_3D_SIZE 2\n");
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                // r·g·b in red; identity in green and blue.
                text.push_str(&format!("{} {g} {b}\n", r * g * b));
            }
        }
    }
    let ops = lut::parse_cube(&text, "x.cube").unwrap();
    let Some(ops::Op::Lut3d { lut, .. }) = ops.first() else {
        panic!("3D LUT");
    };
    let at = [0.5, 0.5, 0.5];
    assert!((lut.apply(at, Interpolation::Linear)[0] - 0.125).abs() < 1e-9);
    // Tetrahedral uses the diagonal: 0.5 · (c111 − c110) = 0.5.
    assert!((lut.apply(at, Interpolation::Tetrahedral)[0] - 0.5).abs() < 1e-9);
    assert!((lut.apply([0.2, 0.7, 0.4], Interpolation::Tetrahedral)[1] - 0.7).abs() < 1e-6);
    // .spi1d with one component, and .spi3d.
    let l = lut::parse_spi1d(
        "Version 1\nFrom 0 1\nLength 2\nComponents 1\n{\n0\n2\n}\n",
        "a.spi1d",
    )
    .unwrap();
    close(l.apply([0.25, 0.5, 1.]), [0.5, 1., 2.], 1e-9);
    let l = lut::parse_spi3d(
        &std::fs::read_to_string(fixture("v2/luts/half.spi3d")).unwrap(),
        "half.spi3d",
    )
    .unwrap();
    close(
        l.apply([1., 0.5, 0.], Interpolation::Linear),
        [0.5, 0.25, 0.],
        1e-9,
    );
    // CLF: blue-fastest LUT3D order, bit-depth scaling, and refused nodes.
    let swap = clf::parse(
        &std::fs::read_to_string(fixture("v2/luts/swap.clf")).unwrap(),
        "swap.clf",
    )
    .unwrap();
    close(
        Processor::new(swap).apply_f64([0.2, 0.5, 0.8]),
        [0.8, 0.5, 0.2],
        1e-6,
    );
    let err = clf::parse(
        &std::fs::read_to_string(fixture("v2/luts/unknown.clf")).unwrap(),
        "unknown.clf",
    )
    .unwrap_err();
    assert!(err.to_string().contains("GradingPrimary"), "{err}");
    let ten_bit = clf::parse(
        r#"<ProcessList id="m"><Matrix inBitDepth="32f" outBitDepth="10i"><Array dim="3 3">1023 0 0 0 1023 0 0 0 1023</Array></Matrix>
        <Exponent inBitDepth="32f" outBitDepth="32f" style="monCurveFwd"><ExponentParams exponent="2.4" offset="0.055"/></Exponent>
        <Log inBitDepth="32f" outBitDepth="32f" style="cameraLinToLog"><LogParams base="2" logSideSlope="0.0570776255707763" logSideOffset="0.554794520547945" linSideBreak="0.0078125"/></Log>
        <ASC_CDL inBitDepth="32f" outBitDepth="32f" style="FwdNoClamp"><SOPNode><Slope>1 1 1</Slope><Offset>0 0 0</Offset><Power>1 1 1</Power></SOPNode><SatNode><Saturation>1</Saturation></SatNode></ASC_CDL></ProcessList>"#,
        "m.clf",
    )
    .unwrap();
    let p = Processor::new(ten_bit);
    // 1023 into 10i is 1.0; sRGB decode of 0.5 → 0.214; then ACEScct.
    let lin = ((0.5 + 0.055) / 1.055f64).powf(2.4);
    let cct = ((lin.log2() + 9.72) / 17.52, 0.);
    close(p.apply_f64([0.5, 0.5, 0.5]), [cct.0; 3], 1e-9);
    // Malformed files say where.
    let err = lut::parse_cube("LUT_3D_SIZE 2\n0 0 0\n", "short.cube").unwrap_err();
    assert!(err.to_string().contains("short.cube"), "{err}");
    let err = lut::parse_cube("LUT_1D_SIZE 2\n0 zero 0\n1 1 1\n", "bad.cube").unwrap_err();
    assert!(err.to_string().contains("line 2"), "{err}");
}

// ── The built-in config and baking ───────────────────────────────────────

#[test]
fn builtin_config_untonemapped_view_is_an_identity_for_srgb_textures() {
    let c = Config::builtin();
    assert_eq!(c.default_display().unwrap().name, "sRGB - Display");
    assert_eq!(
        c.default_view("sRGB - Display").unwrap().name,
        "ACES 1.0 - SDR Video"
    );
    let p = c
        .display_processor("sRGB - Texture", "sRGB - Display", "Un-tone-mapped", None)
        .unwrap();
    for rgb in [
        [0., 0., 0.],
        [1., 1., 1.],
        [0.2, 0.5, 0.8],
        [0.01, 0.99, 0.5],
    ] {
        close(p.apply_f64(rgb), rgb, 1e-6);
    }
    // Every colour space reaches every display and view.
    for cs in c.active_colorspaces() {
        for d in c.active_displays() {
            for v in c.active_views(&d.name) {
                c.display_processor(&cs.name, &d.name, &v.name, None)
                    .unwrap_or_else(|e| panic!("{} → {}/{}: {e}", cs.name, d.name, v.name));
            }
        }
    }
    // ACES view of a white sRGB texture is tone mapped below 1.
    let aces = c
        .display_processor(
            "sRGB - Texture",
            "sRGB - Display",
            "ACES 1.0 - SDR Video",
            None,
        )
        .unwrap();
    let w = aces.apply_f64([1.; 3]);
    assert!(
        w[0] < 0.95 && w[0] > 0.7 && (w[0] - w[1]).abs() < 1e-3,
        "{w:?}"
    );
    // The look and the ACEScct round trip.
    c.display_processor(
        "ACEScg",
        "sRGB - Display",
        "ACES 1.0 - SDR Video",
        Some("ACES 1.3 Reference Gamut Compression"),
    )
    .unwrap();
    let rt = c.processor("ACEScct", "ACEScg").unwrap();
    let back = c.processor("ACEScg", "ACEScct").unwrap();
    close(
        back.apply_f64(rt.apply_f64([0.3, 0.45, 0.6])),
        [0.3, 0.45, 0.6],
        1e-9,
    );
}

#[test]
fn baked_luts_follow_the_exact_ops() {
    let c = Config::builtin();
    let exact = c
        .display_processor(
            "sRGB - Texture",
            "sRGB - Display",
            "ACES 1.0 - SDR Video",
            None,
        )
        .unwrap();
    let baked = exact.bake(65, Shaper::Identity);
    let mut worst: f64 = 0.;
    let mut over = 0;
    let mut seed = 7u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(seed >> 8) / f64::from(1u32 << 24)
    };
    for _ in 0..2000 {
        let rgb = [next(), next(), next()];
        let a = exact.apply_f64(rgb);
        let b = baked.apply_rgb(rgb.map(|v| v as f32));
        for ch in 0..3 {
            let e = (a[ch] - f64::from(b[ch])).abs();
            worst = worst.max(e);
            over += usize::from(e > 0.5 / 255.);
        }
    }
    // Within half an 8-bit code almost everywhere; the clamps at the gamut
    // edges are kinks no lattice follows exactly.
    assert!(
        worst < 1e-2 && over < 30,
        "65³ bake: worst {worst}, {over} of 6000 over half a code"
    );
    // Scene-linear input through a log2 shaper.
    let linear = c
        .display_processor("ACEScg", "sRGB - Display", "ACES 1.0 - SDR Video", None)
        .unwrap();
    let shaped = linear.bake(65, Shaper::Log2 { lo: -12., hi: 4. });
    for v in [0.001, 0.01, 0.18, 1., 4., 12.] {
        let a = linear.apply_f64([v, v * 0.8, v * 0.5]);
        let b = shaped.apply_rgb([v as f32, (v * 0.8) as f32, (v * 0.5) as f32]);
        for ch in 0..3 {
            assert!(
                (a[ch] - f64::from(b[ch])).abs() < 4e-3,
                "{v}: {a:?} vs {b:?}"
            );
        }
    }
    let s = Shaper::Log2 { lo: -12., hi: 4. };
    for x in [0., 1e-5, 0.01, 0.5, 8.] {
        assert!((s.inverse(s.forward(x)) - x).abs() < 1e-9 * x.max(1.));
    }
    // 8-bit paths keep alpha and match the float path.
    let mut rgba = [255u8, 128, 0, 77];
    baked.apply_rgba8(&mut rgba);
    let f = baked.apply_rgb([1., 128. / 255., 0.]);
    assert_eq!(rgba[3], 77);
    assert_eq!(rgba[..3], f.map(|v| (v.clamp(0., 1.) * 255. + 0.5) as u8));
    let mut bgra = [0u8, 128, 255, 9];
    baked.apply_bgra8(&mut bgra);
    assert_eq!(bgra, [rgba[2], rgba[1], rgba[0], 9]);
    let mut exact8 = [255u8, 128, 0, 77];
    exact.apply_rgba8(&mut exact8);
    for ch in 0..3 {
        assert!((i32::from(exact8[ch]) - i32::from(rgba[ch])).abs() <= 1);
    }
}
