//! Stage workflows through the real editor: guides from the board, Camera
//! view, the light table, flip view, the board palette and the mirrored
//! reference view.
use super::*;
use crate::tests::open;
use crate::workspace::Workspace;
use core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_core::storyboard::Panel;
use gpui_kit::test::TestWindowExt;

fn storyboard(panels: usize) -> ProjectEditor {
    let mut p = ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
    if panels > 1 {
        let blank = p.storyboard().unwrap().blank_panel().unwrap();
        let items = (2..=panels)
            .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
            .collect();
        p.insert_panels(Some(1), &blank, items, None).unwrap();
    }
    p
}

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
}

fn setup(
    cx: &mut TestAppContext,
    panels: usize,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
    let editor = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(storyboard(panels), "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    settle(cx);
    (ws, editor, cx)
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
    }
    cx.run_until_parked();
}

fn ids(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<PageId> {
    cx.update(|_, cx| e.read(cx).editor.page_list().iter().map(|m| m.id).collect())
}

/// The composite's sRGB pixel at a document point.
fn pixel(e: &Entity<EditorView>, at: (u32, u32), cx: &mut VisualTestContext) -> [u8; 4] {
    cx.update(|_, cx| {
        let doc = &e.read(cx).editor.doc;
        let flat = emulsion_raster::composite::flatten(&doc.composite_tree(), 0);
        let bytes = flat.to_srgba8();
        let i = ((at.1 * flat.width() + at.0) * 4) as usize;
        bytes[i..i + 4].try_into().unwrap()
    })
}

#[gpui_kit::test]
fn stage_overlays_follow_the_board_and_camera_view_hides_them(cx: &mut TestAppContext) {
    let (_ws, e, cx) = setup(cx, 2);
    cx.update(|window, cx| {
        assert!(window.find("storyboard-stage-toolbar").visible());
        e.update(cx, |e, cx| {
            // Frame plus action and title safe areas, from the board.
            let lines = e.stage_lines();
            assert_eq!(lines.len(), 3);
            assert_eq!(lines[0][2], (64., 36.), "the camera frame");
            assert!(near(lines[1][0], (3.2, 1.8)), "action safe is 90%");
            let paint = e.stage_paint();
            let (area, frame) = paint.pasteboard.expect("10% overscan by default");
            assert!(near((area.x, area.w), (-6.4, 76.8)));
            assert!(near((frame.w, frame.h), (64., 36.)));
            assert_eq!(e.stage_fit_size(), Some((77, 43)));

            // The field guide is board data: one Undo step.
            assert!(e.edit_stage_guides(
                |g| {
                    g.field_guide = true;
                    g.fields = 4;
                },
                cx
            ));
            assert_eq!(e.stage_lines().len(), 3 + 4 + 2, "fields and centre cross");
            e.toggle_safe_areas(cx);
            assert_eq!(e.stage_lines().len(), 1 + 4 + 2);
            e.toggle_safe_areas(cx);
            // Invalid values are refused and leave the board unchanged.
            assert!(!e.edit_stage_guides(|g| g.overscan = 500., cx));
            assert_eq!(e.editor.storyboard().unwrap().stage.overscan, 10.);
            e.undo(cx);
            assert!(!e.editor.storyboard().unwrap().stage.field_guide);
        })
    });
    // Camera view shows only the framed shot.
    cx.update(|window, cx| window.click("stage-camera-view", cx));
    settle(cx);
    cx.update(|_, cx| {
        e.update(cx, |e, _| {
            assert!(e.camera_view());
            assert!(e.stage_lines().is_empty());
            let paint = e.stage_paint();
            assert!(paint.pasteboard.is_none() && paint.light.is_empty());
            assert_eq!(e.stage_fit_size(), None);
        })
    });
    cx.update(|window, cx| {
        assert!(window.try_find("stage-safe-areas").is_none());
        window.dispatch_action(Box::new(crate::actions::ToggleCameraView), cx);
    });
    settle(cx);
    cx.update(|_, cx| assert!(!e.read(cx).camera_view()));
}

#[gpui_kit::test]
fn light_table_picks_neighbours_and_respects_settings(cx: &mut TestAppContext) {
    let (_ws, e, cx) = setup(cx, 5);
    let ids = ids(&e, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.select_page(ids[2], cx);
            assert!(e.light_table_layers(cx).is_empty(), "off by default");
            e.update_light_table(
                |t| {
                    t.enabled = true;
                    t.before = 2;
                    t.after = 1;
                    t.opacity = 0.4;
                },
                cx,
            );
            let layers = e.light_table_layers(cx);
            let order: Vec<_> = layers.iter().map(|l| l.0).collect();
            assert_eq!(order, [ids[0], ids[1], ids[3]], "farthest first");
            assert_eq!(layers[0].1, 0.2);
            assert_eq!(layers[1].2, Some(LightTable::BEFORE_TINT));
            assert_eq!(layers[2].2, Some(LightTable::AFTER_TINT));
            // Invalid settings are refused and not saved.
            e.update_light_table(|t| t.before = 9, cx);
            let saved = &crate::app_state::settings(cx).storyboard.light_table;
            assert_eq!((saved.before, saved.after), (2, 1));
        })
    });
    settle(cx);
    cx.update(|_, cx| {
        e.update(cx, |e, _| {
            let paint = e.stage_paint();
            assert_eq!(paint.light.len(), 3, "one picture per neighbour");
            assert_eq!(paint.light[0].size(0).width.0, 64, "canvas resolution");
        })
    });
    // The shortcut's action turns it off; Camera view hides it.
    cx.update(|window, cx| window.dispatch_action(Box::new(crate::actions::ToggleLightTable), cx));
    settle(cx);
    cx.update(|_, cx| {
        assert!(
            !crate::app_state::settings(cx)
                .storyboard
                .light_table
                .enabled
        );
        e.update(cx, |e, cx| {
            assert!(e.light_table_layers(cx).is_empty());
            e.toggle_light_table(cx);
            e.toggle_camera_view(cx);
            assert!(e.light_table_layers(cx).is_empty());
        })
    });
}

#[gpui_kit::test]
fn flip_view_maps_input_and_leaves_the_art(cx: &mut TestAppContext) {
    let (_ws, e, cx) = setup(cx, 2);
    let ids = ids(&e, cx);
    let (before, plain) = cx.update(|_, cx| {
        let e = e.read(cx);
        (
            (e.editor.revision, e.editor.history.can_undo()),
            e.doc_to_window((12., 9.)).unwrap(),
        )
    });
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(crate::actions::FlipViewHorizontal), cx)
    });
    settle(cx);
    let flipped = cx.update(|_, cx| {
        let e = e.read(cx);
        assert!(e.view.flip_x && !e.view.flip_y);
        assert_eq!(e.editor.revision, before.0, "the art is unchanged");
        assert_eq!(e.editor.history.can_undo(), before.1, "no Undo step");
        let at = e.doc_to_window((12., 9.)).unwrap();
        let back = e.doc_point(at).unwrap();
        assert!((back.0 - 12.).abs() < 1e-6 && (back.1 - 9.).abs() < 1e-6);
        at
    });
    assert_ne!(flipped.x, plain.x, "the point moves to the other side");
    assert_eq!(flipped.y, plain.y);
    // A brush dab lands under the pointer, on the flipped side of the art.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_tool(Tool::Brush, cx);
            e.set_paint(PaintKind::Brush, cx);
            e.set_fg([255, 0, 0, 255], cx);
        })
    });
    cx.simulate_click(flipped, Modifiers::default());
    settle(cx);
    let hit = pixel(&e, (12, 9), cx);
    let mirror = pixel(&e, (52, 9), cx);
    assert!(
        hit[0] > 200 && hit[1] < 80,
        "painted where clicked: {hit:?}"
    );
    assert!(
        mirror[1] > 200 || mirror[3] < 10,
        "not mirrored: {mirror:?}"
    );
    // The flip is a way of looking: it stays on the next panel.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.flip_view(false, cx);
            e.select_page(ids[1], cx);
        })
    });
    settle(cx);
    cx.update(|_, cx| {
        let view = e.read(cx).view;
        assert!(view.flip_x && view.flip_y);
    });
}

#[gpui_kit::test]
fn palette_swatches_paint_and_edits_undo(cx: &mut TestAppContext) {
    let (_ws, e, cx) = setup(cx, 1);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(
                e.board_palette().unwrap(),
                &emulsion_core::storyboard_stage::DEFAULT_PALETTE
            );
            e.tools.picker = true;
            cx.notify();
        })
    });
    settle(cx);
    // The colour picker offers the board palette.
    cx.update(|window, cx| window.click(("sw", 7usize), cx));
    settle(cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(e.tools.fg, [214, 48, 48, 255]);
            e.set_fg([1, 2, 3, 255], cx);
            e.add_palette_color(cx);
            let palette = e.board_palette().unwrap();
            assert_eq!((palette.len(), palette[10]), (11, [1, 2, 3]));
            e.add_palette_color(cx);
            assert_eq!(e.board_palette().unwrap().len(), 11, "no duplicates");
            e.undo(cx);
            assert_eq!(e.board_palette().unwrap().len(), 10, "one Undo step");
            e.remove_palette_color(0, cx);
            assert_eq!(e.board_palette().unwrap()[0], [51, 51, 51]);
            e.undo(cx);
            assert_eq!(e.board_palette().unwrap()[0], [0, 0, 0]);
        })
    });
}

#[gpui_kit::test]
fn reference_mirror_flips_the_preview_not_the_source(cx: &mut TestAppContext) {
    let (_ws, e, cx) = setup(cx, 1);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ref.png");
    image::RgbaImage::from_fn(8, 4, |x, _| {
        if x < 2 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([0, 0, 255, 255])
        }
    })
    .save(&path)
    .unwrap();
    let reference = crate::reference::AttachedReference::load(&path).unwrap();
    let source = reference.image.png().to_vec();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.assistant.reference = Some(reference);
            e.toggle_reference_view(cx);
        })
    });
    settle(cx);
    cx.update(|window, cx| {
        assert!(window.find("storyboard-reference-dock").visible());
        window.click("reference-mirror", cx);
    });
    settle(cx);
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert!(e.stage_ui.reference_mirror);
        let reference = e.assistant.reference.as_ref().unwrap();
        assert_eq!(reference.image.png(), source, "the source is unchanged");
        let (plain, mirrored) = (reference.preview(false), reference.preview(true));
        let (a, b) = (plain.as_bytes(0).unwrap(), mirrored.as_bytes(0).unwrap());
        // Each row reads backwards in the mirror.
        let w = plain.size(0).width.0 as usize;
        let px = |bytes: &[u8], x: usize| bytes[x * 4..x * 4 + 4].to_vec();
        assert_ne!(px(a, 0), px(a, w - 1), "the edges differ");
        assert_eq!(px(b, 0), px(a, w - 1));
        assert_eq!(px(b, w - 1), px(a, 0));
    });
}

#[gpui_kit::test]
fn storyboard_panels_draw_with_paint_brushes_and_symmetry(cx: &mut TestAppContext) {
    let (_ws, e, cx) = setup(cx, 1);
    let at = cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            // Panels open in Paint's tools: brushes with pressure, the brush
            // library (with .abr import) and symmetry.
            assert!(e.draw_mode);
            e.set_tool(Tool::Brush, cx);
            e.set_paint(PaintKind::Brush, cx);
            e.set_fg([255, 0, 0, 255], cx);
            e.tools.mirror_x = true;
            e.doc_to_window((12., 9.)).unwrap()
        })
    });
    cx.simulate_click(at, Modifiers::default());
    settle(cx);
    for x in [12, 52] {
        let p = pixel(&e, (x, 9), cx);
        assert!(p[0] > 200 && p[1] < 80, "mirrored dab at {x}: {p:?}");
    }
}
