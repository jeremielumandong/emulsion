//! Scene integration regressions for the bounds-ordering backport.
use gpui_kit::{Bounds, ContentMask, Quad, ScaledPixels, Scene, point, rgb, size};

fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(width), ScaledPixels(height)),
    )
}

fn paint(scene: &mut Scene, revision: usize, cached: &Scene) {
    let clip = bounds(0., 0., if revision == 2 { 35. } else { 100. }, 100.);
    for i in 0..if revision == 3 { 2 } else { 5 } {
        scene.insert_primitive(Quad {
            bounds: bounds(
                i as f32 * 9. + if revision == 4 { 20. } else { 0. },
                0.,
                30.,
                30.,
            ),
            content_mask: ContentMask { bounds: clip },
            background: rgb(if revision == 1 { 0xff0000 } else { 0x0000ff }).into(),
            ..Quad::default()
        });
    }
    scene.replay(0..cached.len(), cached);
    scene.finish();
}

#[test]
fn gpui_fast_replayed_ordering_preserves_current_paint_clips_and_cached_layers() {
    let mut cached = Scene::default();
    cached.push_layer(bounds(5., 5., 40., 40.));
    cached.insert_primitive(Quad {
        bounds: bounds(10., 10., 30., 30.),
        content_mask: ContentMask {
            bounds: bounds(0., 0., 100., 100.),
        },
        background: rgb(0x00ff00).into(),
        ..Quad::default()
    });
    cached.push_layer(bounds(15., 15., 10., 10.));
    cached.insert_primitive(Quad {
        bounds: bounds(15., 15., 10., 10.),
        content_mask: ContentMask {
            bounds: bounds(0., 0., 100., 100.),
        },
        background: rgb(0xffffff).into(),
        ..Quad::default()
    });
    cached.pop_layer();
    cached.pop_layer();
    cached.finish();
    let mut warm = Scene::default();
    let mut snapshots = Vec::new();
    for revision in [0, 1, 2, 3, 4, 0, 0] {
        warm.clear();
        let mut cold = Scene::default();
        paint(&mut warm, revision, &cached);
        paint(&mut cold, revision, &cached);
        let snapshot = format!("{:?}", warm.quads);
        assert_eq!(snapshot, format!("{:?}", cold.quads), "revision {revision}");
        snapshots.push(snapshot);
    }
    assert_ne!(
        snapshots[0], snapshots[1],
        "current colors must reach paint"
    );
    assert_ne!(snapshots[1], snapshots[2], "current clips must reach paint");
    assert_eq!(snapshots[0], snapshots[5]);
    assert_eq!(snapshots[5], snapshots[6]);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn gpui_fast_ordering_history_does_not_retain_external_textures() {
    use gpui_kit::{ExternalTexture, PaintSurface};
    use std::sync::Arc;
    let mut scene = Scene::default();
    let mut previous: Option<std::sync::Weak<u64>> = None;
    for revision in 0_u64..4 {
        scene.clear();
        if let Some(previous) = &previous {
            assert!(
                previous.upgrade().is_none(),
                "bounds history must not own textures"
            );
        }
        let texture = Arc::new(revision);
        previous = Some(Arc::downgrade(&texture));
        scene.insert_primitive(PaintSurface {
            order: 0,
            bounds: bounds(0., 0., 20., 20.),
            content_mask: ContentMask {
                bounds: bounds(0., 0., 100., 100.),
            },
            texture: ExternalTexture(texture),
        });
        scene.finish();
        assert_eq!(
            scene.surfaces[0].texture.0.downcast_ref::<u64>(),
            Some(&revision)
        );
    }
    let mut cached = Scene::default();
    cached.replay(0..scene.len(), &scene);
    scene.clear();
    let previous = previous.unwrap();
    assert!(
        previous.upgrade().is_some(),
        "explicit cached paint owns its texture"
    );
    cached.clear();
    assert!(
        previous.upgrade().is_none(),
        "bounds history must not own textures"
    );
}
