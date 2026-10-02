use super::*;
use crate::raw_fixture;
use emulsion_core::Editor;

fn px(rgb: [f32; 3]) -> [u16; 4] {
    // Display values in, linear-light storage out, as documents hold pixels.
    let linear = rgb.map(|v| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    [
        (linear[0] * 65535.) as u16,
        (linear[1] * 65535.) as u16,
        (linear[2] * 65535.) as u16,
        65535,
    ]
}

#[test]
fn analysis_measures_cast_skin_tone_and_key() {
    let warm_grey = analyze_pixels(64, 64, |x, _| {
        let v = 0.3 + x as f32 / 64. * 0.4;
        px([v * 1.06, v, v * 0.92])
    });
    assert!(warm_grey.cast.warmth > 0.08, "{:?}", warm_grey.cast);
    assert!(warm_grey.has("flat"));

    let skin = analyze_pixels(64, 64, |_, _| px([0.85, 0.62, 0.5]));
    assert!(skin.has("portrait"), "{:?}", skin.scene);
    assert!(skin.hue_share["orange"] > 50.);

    let night = analyze_pixels(64, 64, |x, y| {
        if x < 10 && y < 10 {
            px([1., 0.9, 0.6])
        } else {
            px([0.05, 0.06, 0.12])
        }
    });
    assert!(
        night.has("night") && night.has("low_key"),
        "{:?}",
        night.scene
    );
    assert_eq!(night.tone.key, "low");
}

#[test]
fn every_look_is_valid_at_any_strength_and_zero_strength_is_identity() {
    let analyses = [
        Analysis::default(),
        analyze_pixels(32, 32, |_, _| px([0.85, 0.62, 0.5])),
        analyze_pixels(32, 32, |x, _| px([x as f32 / 32., 0.2, 0.9])),
    ];
    for look in LOOKS {
        for a in &analyses {
            let base = DevelopParams::default();
            let mut styled = base;
            (look.build)(&mut styled);
            adapt(&mut styled, a, true);
            for strength in [0., 0.5, 1., 1.5] {
                let out = blend(&base, &styled, strength);
                out.validate()
                    .unwrap_or_else(|e| panic!("{} at {strength}: {e}", look.key));
                assert!(out.tone_curve.windows(2).all(|w| w[0] <= w[1]));
            }
            assert_eq!(blend(&base, &styled, 0.), base);
        }
    }
    let mut keys: Vec<_> = LOOKS.iter().map(|l| l.key).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), LOOKS.len());
}

#[test]
fn requests_resolve_by_auto_key_and_mood_words() {
    let skin = analyze_pixels(32, 32, |_, _| px([0.85, 0.62, 0.5]));
    let (auto, why) = resolve("auto", &skin).unwrap();
    let Chosen::Look(auto) = auto else {
        panic!("auto picks an adaptive look")
    };
    assert!(auto.suits.contains(&"portrait"), "{}", auto.key);
    assert!(why.contains("portrait"));
    assert_eq!(resolve("noir-bw", &skin).unwrap().0.key(), "noir-bw");
    assert_eq!(
        resolve("Dark & Moody", &skin).unwrap().0.key(),
        "moody-dark"
    );
    assert_eq!(
        resolve("dark and moody please", &skin).unwrap().0.key(),
        "moody-dark"
    );
    assert_eq!(
        resolve("teal orange movie", &skin).unwrap().0.key(),
        "cinematic-teal-orange"
    );
    assert!(
        resolve("black and white", &skin)
            .unwrap()
            .0
            .key()
            .ends_with("-bw")
    );
    assert!(resolve("zzz", &skin).is_err());
    // Monochrome looks are never offered uninvited for a colourful photo.
    let colourful = analyze_pixels(32, 32, |x, _| px([x as f32 / 32., 0.2, 0.9]));
    assert!(
        !resolve("auto", &colourful)
            .unwrap()
            .0
            .key()
            .ends_with("-bw")
    );
}

#[test]
fn applying_looks_is_one_undo_step_and_never_stacks() {
    let dir = std::env::temp_dir().join(format!("emulsion-mcp-raw-looks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("source.dng");
    raw_fixture::write_dng(&path);
    let original = std::fs::read(&path).unwrap();
    let mut editor = Editor::new(emulsion_io::raw::open(&path).unwrap(), None);
    let before = editor.doc.clone();

    let analysis = crate::exec::execute(&mut editor, "analyze_raw", &json!({}));
    assert!(!analysis.is_error, "{analysis:?}");
    let text: Value = serde_json::from_str(analysis.content[0]["text"].as_str().unwrap()).unwrap();
    assert!(text["recommended_looks"].as_array().unwrap().len() >= 3);
    assert!(crate::exec::execute(&mut editor, "analyze_raw", &json!({"x":1})).is_error);
    let listed = crate::exec::execute(&mut editor, "list_raw_looks", &json!({}));
    assert!(!listed.is_error);
    assert_eq!(editor.doc, before);

    for args in [
        json!({"look":"zzz"}),
        json!({"strength":2}),
        json!({"correct":"yes"}),
        json!({"mood":"warm"}),
    ] {
        assert!(
            crate::raw_tools::plan(&editor.doc, "apply_raw_look", &args).is_err(),
            "{args}"
        );
    }

    let p = crate::raw_tools::plan(
        &editor.doc,
        "apply_raw_look",
        &json!({"look":"vintage faded"}),
    )
    .unwrap();
    let message: Value = serde_json::from_str(&p.message).unwrap();
    assert_eq!(message["look"], "vintage-faded");
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    assert_eq!(editor.history.len(), 1);
    let vintage = editor.doc.raw.as_ref().unwrap().params;
    assert!(vintage.point_curves[3].len > 0);

    let p = crate::raw_tools::plan(
        &editor.doc,
        "apply_raw_look",
        &json!({"look":"classic-bw","correct":false,"strength":1}),
    )
    .unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let bw = editor.doc.raw.as_ref().unwrap().params;
    assert_eq!(bw.saturation, -1.);
    // The vintage blue curve and grading were replaced, not stacked.
    assert_eq!(bw.point_curves[3].len, 0);
    assert_eq!(bw.grading, [[0.; 3]; 3]);
    // correct=false keeps exposure and white balance.
    assert_eq!(bw.exposure, vintage.exposure);
    assert_eq!(bw.temperature, vintage.temperature);

    editor.undo();
    editor.undo();
    assert_eq!(editor.doc, before);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn harmonies_follow_the_colour_wheel_and_protect_skin() {
    assert_eq!(Harmony::Complementary.hues(30.), vec![30., 200.]);
    assert_eq!(Harmony::Triadic.hues(300.), vec![300., 60., 180.]);
    let mut p = DevelopParams::default();
    let note = apply_harmony(&mut p, Harmony::Complementary, 30.).unwrap();
    assert!(note.contains("Complementary"));
    // Warm hue tones highlights, its complement tones shadows.
    assert_eq!(p.grading[2][0], 30.);
    assert_eq!(p.grading[0][0], 200.);
    // Orange is on-palette; green is off-palette and muted, shifted toward a scheme hue.
    assert!(p.hsl[O][1] > 0.);
    assert!(p.hsl[G][1] < -0.2 && p.hsl[G][0] != 0.);
    p.validate().unwrap();

    let mut mono = DevelopParams {
        saturation: -1.,
        ..DevelopParams::default()
    };
    assert!(apply_harmony(&mut mono, Harmony::Triadic, 0.).is_none());

    // Green-cast skin is steered back toward the flattering range in the orange band.
    let greenish = analyze_pixels(32, 32, |_, _| px([0.75, 0.68, 0.5]));
    assert!(greenish.has("portrait"), "{:?}", greenish);
    let mut p = DevelopParams::default();
    apply_harmony(&mut p, Harmony::Complementary, 120.);
    let notes = adapt(&mut p, &greenish, true);
    assert!(p.hsl[O][0] < 0., "{:?} {notes:?}", p.hsl[O]);
    assert!(p.hsl[O][1] <= 0.05 && p.grading[1][1] <= 0.05);
    assert!(notes.iter().any(|n| n.contains("skin hue")));
    // Without a clean measurement only protection applies.
    let mut q = DevelopParams::default();
    adapt(&mut q, &greenish, false);
    assert_eq!(q.hsl[O][0], 0.);
}

#[test]
fn black_and_white_uses_scene_filters_and_landscapes_get_sky_rules() {
    let sky = analyze_pixels(32, 32, |_, y| {
        if y < 14 {
            px([0.35, 0.55, 0.9])
        } else {
            px([0.3, 0.55, 0.2])
        }
    });
    assert!(sky.has("landscape") && sky.sky_pct >= 8., "{sky:?}");
    let mut bw = DevelopParams::default();
    (LOOKS.iter().find(|l| l.key == "classic-bw").unwrap().build)(&mut bw);
    let notes = adapt(&mut bw, &sky, true);
    assert!(notes[0].contains("dramatic sky"), "{notes:?}");
    // A true B&W mixer, not calibration, which the style guide rules out.
    assert!(bw.gray_mixer[B] < 0. && bw.gray_mixer[Y] > 0.);
    assert_eq!(bw.calibration, DevelopParams::default().calibration);
    bw.validate().unwrap();

    let mut colour = DevelopParams::default();
    adapt(&mut colour, &sky, true);
    assert!(colour.hsl[B][2] < 0. && colour.hsl[Y][1] < 0.);

    assert!(add_sky_grad(&mut colour, false));
    assert!(colour.masks.iter().any(is_sky_grad));
    colour.validate().unwrap();
    clear_look(&mut colour);
    assert!(!colour.masks.iter().any(|m| m.enabled));
}

#[test]
fn palette_mood_reads_colour_psychology() {
    let warm = analyze_pixels(32, 32, |_, _| px([0.9, 0.55, 0.2]));
    assert!(
        palette_mood(&warm).contains(&"energetic"),
        "{:?}",
        palette_mood(&warm)
    );
    let cool = analyze_pixels(32, 32, |_, _| px([0.2, 0.4, 0.8]));
    let mood = palette_mood(&cool);
    assert!(
        mood.contains(&"calm") && mood.iter().any(|m| m.starts_with("blue")),
        "{mood:?}"
    );
    // A cheerful portrait is not steered into the dark, moody grade.
    let skin = analyze_pixels(32, 32, |_, _| px([0.85, 0.62, 0.5]));
    assert_ne!(resolve("auto", &skin).unwrap().0.key(), "moody-dark");
    assert_eq!(
        resolve("calm peaceful", &cool).unwrap().0.key(),
        "nordic-cool"
    );
    assert_eq!(resolve("sepia", &cool).unwrap().0.key(), "vintage-faded");
}

#[test]
fn film_presets_resolve_by_name_and_words_but_moods_stay_adaptive() {
    let a = Analysis::default();
    let name = |request: &str| resolve(request, &a).unwrap().0.key();
    assert_eq!(name("Kodak Portra 400"), "Kodak Portra 400");
    assert_eq!(name("portra 400"), "Kodak Portra 400");
    assert_eq!(name("tri-x"), "Kodak Tri-X 400");
    assert_eq!(name("cinestill 800t"), "Cinestill 800T");
    assert!(matches!(
        resolve("Fuji Velvia 50", &a).unwrap().0,
        Chosen::Film(_)
    ));
    // Mood words keep using the adaptive looks.
    assert_eq!(name("cinematic"), "cinematic-teal-orange");
    assert_eq!(name("dark and moody"), "moody-dark");
    // Every recommended stock exists in the bundled library.
    let all_tags = Analysis {
        scene: vec![
            "portrait",
            "golden_light",
            "landscape",
            "night",
            "monochrome",
            "high_key",
            "muted",
            "flat",
            "contrasty",
        ],
        ..Analysis::default()
    };
    let recs = film_recommendations(&all_tags);
    assert_eq!(recs.len(), 5);
    assert!(
        recs.iter()
            .all(|r| film_library::find(r["name"].as_str().unwrap()).is_some())
    );
    assert!(!film_recommendations(&Analysis::default()).is_empty());
}

#[test]
fn style_guide_limits_hold_for_every_look() {
    let a = Analysis {
        scene: vec!["portrait"],
        ..Analysis::default()
    };
    let mut p = DevelopParams {
        grain: [0.4, 0.3, 0.5],
        sharpening: 0.5,
        clarity: 0.2,
        texture: 0.1,
        dehaze: 0.1,
        blacks: 0.2,
        tone_curve: [0.08, 0.3, 0.55, 0.8, 0.97],
        saturation: -0.3,
        vibrance: 0.2,
        shadows: 0.2,
        grading: [[210., 0.5, -0.2], [30., 0.4, 0.], [45., 0.6, 0.]],
        ..DevelopParams::default()
    };
    p.hsl[G][1] = -0.9;
    let notes = style_guide(&mut p, &a);
    assert!(p.sharpening <= 10. / 150. + 1e-6);
    assert_eq!((p.clarity, p.texture, p.dehaze), (0., 0., 0.));
    assert_eq!(p.blacks, 0.);
    assert!((p.vibrance - p.saturation).abs() <= 0.1 + 1e-6);
    assert_eq!(p.hsl[G][1], -0.6);
    assert!(p.grading.iter().all(|w| w[1] <= 0.3));
    assert!(p.grading[1][1] <= 0.1);
    assert_eq!(p.grading[0][2], 0.);
    assert!(notes.len() >= 4, "{notes:?}");
    p.validate().unwrap();

    // Without grain, only one of the local-contrast trio leads.
    let mut q = DevelopParams {
        clarity: 0.2,
        texture: 0.1,
        dehaze: 0.1,
        ..DevelopParams::default()
    };
    style_guide(&mut q, &Analysis::default());
    assert_eq!(q.clarity, 0.2);
    assert_eq!((q.texture, q.dehaze), (0.05, 0.05));
}

#[test]
fn film_library_presets_apply_through_the_adaptive_pipeline() {
    let dir = std::env::temp_dir().join(format!("emulsion-mcp-film-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("source.dng");
    raw_fixture::write_dng(&path);
    let mut editor = Editor::new(emulsion_io::raw::open(&path).unwrap(), None);
    let before = editor.doc.clone();

    let listed = list(&json!({"query":"portra"})).unwrap();
    let text: Value = serde_json::from_str(listed.content[0]["text"].as_str().unwrap()).unwrap();
    assert!(text["film_library"].as_array().unwrap().len() >= 3);
    assert!(list(&json!({"bogus":1})).is_err());

    let p = crate::raw_tools::plan(
        &editor.doc,
        "apply_raw_look",
        &json!({"look":"Kodak Tri-X 400"}),
    )
    .unwrap();
    let message: Value = serde_json::from_str(&p.message).unwrap();
    assert_eq!(message["look"], "Kodak Tri-X 400");
    assert_eq!(message["source"], "Black-White");
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let tri_x = editor.doc.raw.as_ref().unwrap().params;
    assert_eq!(tri_x.saturation, -1.);
    assert!(tri_x.grain[0] > 0.);
    assert!(tri_x.sharpening <= 10. / 150. + 1e-6);
    assert_eq!(editor.history.len(), 1);

    // A colour stock replaces the B&W one instead of stacking on it.
    let p = crate::raw_tools::plan(
        &editor.doc,
        "apply_raw_look",
        &json!({"look":"portra 400","strength":0.8}),
    )
    .unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let portra = editor.doc.raw.as_ref().unwrap().params;
    assert!(portra.saturation > -1.);
    assert_eq!(portra.gray_mixer, [0.; 8]);

    editor.undo();
    editor.undo();
    assert_eq!(editor.doc, before);
    std::fs::remove_dir_all(dir).unwrap();
}
