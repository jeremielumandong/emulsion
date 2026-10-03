//! The Shot Generator through the real editor: open it on a panel, add a
//! character, pose it and use the set as the panel's reference layer.
use super::*;
use crate::tests::open;
use crate::workspace::Workspace;
use core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_core::storyboard_shot::REFERENCE_LAYER;
use gpui_kit::test::TestWindowExt;

fn setup(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
    let editor = cx.update(|window, cx| {
        let project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    settle(cx);
    (ws, editor, cx)
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
    }
    cx.run_until_parked();
}

fn characters(
    e: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<emulsion_scene::Character> {
    cx.update(|_, cx| {
        let e = e.read(cx);
        let panel = e.editor.active_page();
        e.editor
            .panel_shot(panel)
            .map(|s| {
                s.set
                    .objects
                    .iter()
                    .filter_map(|o| o.character().cloned())
                    .collect()
            })
            .unwrap_or_default()
    })
}

#[gpui_kit::test]
fn shot_generator_builds_a_posed_reference_layer(cx: &mut TestAppContext) {
    let (_ws, e, cx) = setup(cx);
    // The Stage toolbar's 3D button opens it in place of the Stage.
    cx.update(|window, cx| window.click("stage-shot-generator", cx));
    settle(cx);
    cx.update(|window, cx| {
        assert!(e.read(cx).shot_generator_open());
        assert!(window.find("shot-generator").visible());
        assert!(window.find("shot-viewport").visible());
    });
    // Add an adult female mannequin: one Undo step.
    cx.update(|window, cx| window.click(("shot-add-character", 1usize), cx));
    settle(cx);
    let added = characters(&e, cx);
    assert_eq!(added.len(), 1);
    assert_eq!(
        added[0].body.kind,
        emulsion_scene::MannequinKind::AdultFemale
    );
    // The viewport rendered off the UI thread.
    cx.update(|_, cx| {
        let generator = e.read(cx).shot_generator.clone().unwrap();
        assert!(generator.read(cx).frame.is_some(), "a viewport frame");
    });
    // Choose the Wave pose for the selected character.
    let wave = emulsion_scene::PosePreset::ALL
        .iter()
        .position(|p| *p == emulsion_scene::PosePreset::Wave)
        .unwrap();
    cx.update(|window, cx| window.click(("shot-pose", wave), cx));
    settle(cx);
    assert_eq!(characters(&e, cx)[0].pose.name, "wave");
    // Use it as the panel's reference layer.
    cx.update(|window, cx| window.click("shot-use-reference", cx));
    settle(cx);
    let layer = cx.update(|_, cx| {
        let e = e.read(cx);
        let node = e
            .editor
            .doc
            .nodes
            .iter()
            .find(|n| n.name == REFERENCE_LAYER)
            .cloned()
            .expect("a reference layer");
        assert!(node.locked && node.opacity < 1.);
        let NodeKind::Raster { raster, .. } = &node.kind else {
            panic!("a raster layer");
        };
        assert_eq!((raster.width(), raster.height()), (64, 36));
        node.id
    });
    // Undo takes the pose back (the reference joined the pose step).
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    settle(cx);
    assert_eq!(characters(&e, cx)[0].pose.name, "stand");
    cx.update(|_, cx| e.update(cx, |e, cx| e.redo(cx)));
    settle(cx);
    assert!(cx.update(|_, cx| e.read(cx).editor.doc.node(layer).is_some()));
    // Done goes back to the Stage.
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(crate::actions::ToggleShotGenerator), cx)
    });
    settle(cx);
    cx.update(|window, cx| {
        assert!(!e.read(cx).shot_generator_open());
        assert!(window.find("storyboard-stage-toolbar").visible());
    });
}

#[gpui_kit::test]
fn describing_a_shot_and_the_explorer(cx: &mut TestAppContext) {
    let (_ws, e, cx) = setup(cx);
    cx.update(|_, cx| e.update(cx, |e, cx| e.toggle_shot_generator(cx)));
    settle(cx);
    let generator = cx.update(|_, cx| e.read(cx).shot_generator.clone().unwrap());
    cx.update(|_, cx| {
        generator.update(cx, |g, cx| {
            g.describe_text("wide shot of two people talking", cx)
        })
    });
    settle(cx);
    assert_eq!(characters(&e, cx).len(), 2);
    cx.update(|window, cx| window.click("shot-explore", cx));
    settle(cx);
    let (count, thumbs) = cx.update(|_, cx| {
        let g = generator.read(cx);
        let explorer = g.explorer.as_ref().expect("proposals");
        (
            explorer.proposals.len(),
            explorer.thumbs.iter().filter(|t| t.is_some()).count(),
        )
    });
    assert!(count >= 6);
    assert_eq!(thumbs, count, "every proposal has a thumbnail");
    let chosen =
        cx.update(|_, cx| generator.read(cx).explorer.as_ref().unwrap().proposals[2].camera);
    cx.update(|window, cx| window.click(("shot-proposal", 2usize), cx));
    settle(cx);
    let camera = cx.update(|_, cx| {
        let e = e.read(cx);
        e.editor
            .panel_shot(e.editor.active_page())
            .unwrap()
            .set
            .camera
    });
    assert_eq!(camera, chosen);
}
