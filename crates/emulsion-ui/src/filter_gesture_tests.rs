//! A completed preview and an asynchronous final filter value share one undo.
use super::*;
use emulsion_core::NodeKind;
use emulsion_filters::Filter;

#[gpui_kit::test]
fn smart_filter_preview_and_final_render_form_one_undo_step(cx: &mut TestAppContext) {
    let source = Raster::from_fn(256, 192, [0; 4], |x, _| {
        if x < 128 {
            [65535, 0, 0, 65535]
        } else {
            [0, 0, 65535, 65535]
        }
    });
    let mut document = doc(&["Photo"], Some(source));
    let id = document.nodes[0].id;
    Command::ConvertToSmart { id }.apply(&mut document).unwrap();
    Command::SetFilters {
        id,
        filters: vec![Filter::GaussianBlur { radius: 1.0 }],
    }
    .apply(&mut document)
    .unwrap();
    let before = document.clone();
    let before_pixel =
        emulsion_raster::composite::flatten(&before.composite_tree(), 0).get(132, 96);
    let (ws, cx) = open(cx, document);
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());

    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.editor.begin("Filter radius");
            let NodeKind::Smart { source, .. } = &e.editor.doc.node(id).unwrap().kind else {
                panic!("smart layer");
            };
            let preview_filters = vec![Filter::GaussianBlur { radius: 3.0 }];
            let (cache, offset) = emulsion_core::smart::render(source, &preview_filters);
            // Install the same command a completed worker uses during a held slider.
            e.execute(
                Command::SetSmartCache {
                    id,
                    filters: preview_filters,
                    cache,
                    offset,
                },
                cx,
            );
            assert_ne!(e.editor.doc, before, "the intermediate preview is visible");
            assert_eq!(
                e.editor.history.len(),
                0,
                "preview remains inside the gesture"
            );

            // Release while a newer final value is queued. finish_filter_gesture
            // snapshots it, cancels the preview, and schedules the sole commit.
            e.set_filter_param(id, 0, "radius", 6.0, true, cx);
            e.finish_filter_gesture(id, cx);
            assert!(!e.editor.in_transaction());
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.history.len(), 0);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let NodeKind::Smart { filters, .. } = &e.editor.doc.node(id).unwrap().kind else {
                panic!("smart layer");
            };
            assert_eq!(filters, &[Filter::GaussianBlur { radius: 6.0 }]);
            let after_pixel =
                emulsion_raster::composite::flatten(&e.editor.doc.composite_tree(), 0).get(132, 96);
            assert_ne!(
                after_pixel, before_pixel,
                "the final filter changes the rendered image"
            );
            assert_eq!(
                e.editor.history.len(),
                1,
                "the preview must not add a separate undo step"
            );
            e.undo(cx);
            assert_eq!(
                e.editor.doc, before,
                "one undo restores pre-gesture filters and cache"
            );
            assert_eq!(e.editor.history.len(), 0);
        })
    });
}
