//! Saved adjustment workflows use the same editable preview and undo path.
use super::*;
use emulsion_core::NodeKind;
use emulsion_raster::{Adjustment, composite::flatten};
use emulsion_recipes::{Recipe, capture_adjustments};

fn photo() -> Document {
    doc(
        &["Photo"],
        Some(Raster::solid(256, 192, [0.12, 0.3, 0.08, 1.0])),
    )
}

fn workflow(name: &str, exposure: f32) -> (Recipe, Document) {
    let mut d = photo();
    let group = Command::AddNode {
        node: Box::new(Node::group(0, "Custom grade")),
        slot: Slot::TOP,
    }
    .apply(&mut d)
    .unwrap()
    .unwrap();
    for adjustment in [
        Adjustment::Exposure {
            exposure,
            offset: 0.03,
            gamma: 1.2,
        },
        Adjustment::HueSaturation {
            hue: 18.0,
            saturation: -24.0,
            lightness: 3.0,
        },
    ] {
        let mut node = Node::adjust(0, adjustment);
        node.opacity = 0.65;
        Command::AddNode {
            node: Box::new(node),
            slot: Slot::top_of(Some(group)),
        }
        .apply(&mut d)
        .unwrap();
    }
    let recipe = capture_adjustments(&d, group, name, &[]).unwrap();
    // Exercise the saved representation, not only the in-memory capture.
    (Recipe::from_toml(&recipe.to_toml()).unwrap(), d)
}

#[gpui_kit::test]
fn workflow_preview_replacement_cancel_and_apply_preserve_pixels_and_single_undo(
    cx: &mut TestAppContext,
) {
    let before = photo();
    let base_pixels = flatten(&before.composite_tree(), 0).to_srgba8();
    let (first, first_doc) = workflow("Warm custom grade", 0.4);
    let (second, second_doc) = workflow("Darker custom grade", -0.6);
    let first_pixels = flatten(&first_doc.composite_tree(), 0).to_srgba8();
    let second_pixels = flatten(&second_doc.composite_tree(), 0).to_srgba8();
    assert_ne!(first_pixels, base_pixels);
    assert_ne!(first_pixels, second_pixels);
    let (ws, cx) = open(cx, before.clone());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.preview_recipe(&first, cx);
            assert!(e.recipes.preview.is_some() && e.editor.in_transaction());
            assert!(e.editor.history.is_empty());
            assert_eq!(
                flatten(&e.editor.doc.composite_tree(), 0).to_srgba8(),
                first_pixels
            );
            e.preview_recipe(&second, cx);
            assert_eq!(
                e.editor.doc.nodes.len(),
                second_doc.nodes.len(),
                "preview replaces earlier stages"
            );
            assert_eq!(
                flatten(&e.editor.doc.composite_tree(), 0).to_srgba8(),
                second_pixels
            );
            e.cancel_preview(cx);
            assert_eq!(e.editor.doc, before);
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.in_transaction());

            e.preview_recipe(&first, cx);
            e.apply_preview(cx);
            assert!(e.recipes.preview.is_none());
            assert!(!e.editor.in_transaction());
            assert_eq!(e.editor.history.len(), 1);
            let group = e.selected.expect("applied workflow selected");
            let stages = e.editor.doc.children(Some(group));
            assert_eq!(stages.len(), 2);
            assert!(
                stages
                    .iter()
                    .all(|id| matches!(e.editor.doc.node(*id).unwrap().kind, NodeKind::Adjust(_)))
            );
            assert_eq!(
                flatten(&e.editor.doc.composite_tree(), 0).to_srgba8(),
                first_pixels
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
            e.redo(cx);
            assert_eq!(
                flatten(&e.editor.doc.composite_tree(), 0).to_srgba8(),
                first_pixels
            );
        });
    });
}

#[gpui_kit::test]
fn workflow_preview_does_not_take_over_an_active_edit(cx: &mut TestAppContext) {
    let (recipe, _) = workflow("Saved grade", 0.4);
    let before = photo();
    let (ws, cx) = open(cx, before.clone());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let id = e.editor.doc.nodes[0].id;
            e.editor.begin("Unfinished edit");
            e.execute(Command::SetOpacity { id, opacity: 0.5 }, cx);
            let preview = e.editor.doc.clone();
            e.preview_recipe(&recipe, cx);
            assert_eq!(e.editor.doc, preview);
            assert!(e.recipes.preview.is_none());
            assert!(e.editor.in_transaction());
            assert!(e.editor.history.is_empty());
            e.apply_recipe(&recipe, cx);
            assert_eq!(e.editor.doc, preview);
            assert!(e.editor.in_transaction());
            assert!(e.editor.history.is_empty());
            e.editor.cancel();
            assert_eq!(e.editor.doc, before);
        });
    });
}

#[gpui_kit::test]
fn failed_preview_replacement_restores_the_displayed_picture(cx: &mut TestAppContext) {
    let before = photo();
    let original_pixels = flatten(&before.composite_tree(), 0).to_srgba8();
    let (good, _) = workflow("Good workflow", 0.7);
    let missing = std::env::temp_dir().join(format!(
        "emulsion-missing-lut-{}-{}.cube",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let broken = Recipe {
        name: "Missing LUT".into(),
        lut: Some(missing.to_string_lossy().into()),
        ..Recipe::default()
    };
    let (ws, cx) = open(cx, before.clone());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.preview_recipe(&good, cx);
            assert!(e.recipes.preview.is_some());
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_ne!(flatten(&e.tree, 0).to_srgba8(), original_pixels);
            e.preview_recipe(&broken, cx);
            assert!(e.recipes.preview.is_none());
            assert_eq!(e.editor.doc, before);
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.in_transaction());
            assert!(e.status.as_ref().is_some_and(|(_, error)| *error));
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(flatten(&e.read(cx).tree, 0).to_srgba8(), original_pixels);
    });
}

#[gpui_kit::test]
fn capture_form_preserves_metadata_and_exclusions_without_editing_artwork(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;
    let (_, d) = workflow("Source", 0.4);
    let group = d.nodes.iter().find(|n| n.is_group()).unwrap().id;
    let before = d.clone();
    let (ws, cx) = open(cx, d);
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let recipe = cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            e.selected = Some(group);
            e.toggle_recipes(cx);
            e.begin_recipe_capture(window, cx);
            let draft = e.recipes.capture.as_mut().expect("capture form");
            draft.name.update(cx, |input, cx| {
                input.set_value("My portable grade", window, cx)
            });
            draft.tags.update(cx, |input, cx| {
                input.set_value("portrait, warm, portrait", window, cx)
            });
            draft.notes.update(cx, |input, cx| {
                input.set_value("Keep skin tones gentle", window, cx)
            });
            assert_eq!(draft.stages.len(), 2);
            let excluded = draft.stages[0].0;
            draft.stages[0].2 = false;
            let recipe = e.captured_recipe(cx).unwrap();
            assert_eq!(recipe.name, "My portable grade");
            assert_eq!(recipe.tags, ["portrait", "warm"]);
            assert_eq!(recipe.notes, "Keep skin tones gentle");
            let stages = &recipe.workflow.as_ref().unwrap().stages;
            assert_eq!(stages.len(), 1);
            let kept = before
                .children(Some(group))
                .into_iter()
                .find(|id| *id != excluded)
                .unwrap();
            let NodeKind::Adjust(expected) = &before.node(kept).unwrap().kind else {
                panic!("adjustment")
            };
            assert_eq!(&stages[0].adjustment, expected);
            assert_eq!(e.editor.doc, before);
            assert!(e.editor.history.is_empty());
            recipe
        })
    });
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find("rc-capture-form").visible()));
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "emulsion-ui-workflow-{}-{unique}",
        std::process::id()
    ));
    let saved = emulsion_recipes::store::save_new(&dir, &recipe).unwrap();
    assert_eq!(
        Recipe::from_toml(&std::fs::read_to_string(saved).unwrap()).unwrap(),
        recipe
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[gpui_kit::test]
fn capture_rejects_pixel_selections_and_stale_or_busy_drafts(cx: &mut TestAppContext) {
    let (_, d) = workflow("Source", 0.4);
    let group = d.nodes.iter().find(|n| n.is_group()).unwrap().id;
    let pixels = d
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Raster { .. }))
        .unwrap()
        .id;
    let before = d.clone();
    let (ws, cx) = open(cx, d);
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            e.selected = Some(pixels);
            e.begin_recipe_capture(window, cx);
            assert!(e.recipes.capture.is_none());
            assert!(e.status.as_ref().is_some_and(|(_, error)| *error));
            assert_eq!(e.editor.doc, before);
            assert!(e.editor.history.is_empty());

            e.selected = Some(group);
            e.editor.begin("Active edit");
            e.begin_recipe_capture(window, cx);
            assert!(e.recipes.capture.is_none());
            assert!(e.editor.in_transaction());
            e.editor.cancel();
            e.begin_recipe_capture(window, cx);
            assert!(e.captured_recipe(cx).is_ok());
            e.selected = Some(pixels);
            assert!(e.captured_recipe(cx).is_err());
            e.selected = Some(group);
            e.editor.begin("Active edit");
            assert!(e.captured_recipe(cx).is_err());
            e.editor.cancel();
            e.execute(
                Command::SetOpacity {
                    id: group,
                    opacity: 0.5,
                },
                cx,
            );
            assert!(
                e.captured_recipe(cx).is_err(),
                "old capture cannot save changed stages"
            );
            assert_eq!(
                e.editor.history.len(),
                1,
                "capture never creates history entries"
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
        });
    });
}

#[gpui_kit::test]
fn batch_shows_legacy_rendering_limitations_beside_selected_recipe(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;
    let legacy = Recipe {
        name: "Imported camera recipe".into(),
        sharpness: 2.0,
        noise_reduction: -2.0,
        color_chrome_fx_blue: emulsion_recipes::Strength::Weak,
        ..Recipe::default()
    };
    let (exact, _) = workflow("Exact workflow", 0.4);
    let (ws, cx) = open(cx, photo());
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.batch.recipe = Some(legacy.name.clone());
            ws.batch.recipes = Some(vec![legacy, exact.clone()]);
            ws.screen = crate::workspace::Screen::Batch;
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find("batch-recipe-limitations").visible()));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.batch.recipe = Some(exact.name.clone());
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.try_find("batch-recipe-limitations").is_none()));
}
