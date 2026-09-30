//! Scene integration regressions for bounds ordering, sorting and atlas batching.

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
        for x in [30., 0., 15.] {
            scene.insert_primitive(PaintSurface {
                order: 0,
                bounds: bounds(x, 0., 20., 20.),
                content_mask: ContentMask {
                    bounds: bounds(0., 0., 100., 100.),
                },
                texture: ExternalTexture(texture.clone()),
            });
        }
        drop(texture);
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

#[test]
fn gpui_fast_scene_sort_matches_stable_order_across_reused_frames() {
    let mut scene = Scene::default();
    let mut random = 0x5eed_u64;
    for count in [512, 1, 64, 0, 127, 128, 129, 1023, 1024, 1025, 32, 512] {
        scene.clear();
        for index in 0..count {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            scene.quads.push(Quad {
                order: (random % 16) as u32,
                bounds: bounds(index as f32, 0., 10., 10.),
                background: rgb(index).into(),
                ..Default::default()
            });
        }
        let mut expected = scene.quads.clone();
        expected.sort_by_key(|quad| quad.order);
        scene.finish();
        assert_eq!(format!("{:?}", scene.quads), format!("{expected:?}"));
        scene.finish();
        assert_eq!(format!("{:?}", scene.quads), format!("{expected:?}"));
    }
}

#[test]
fn gpui_fast_scene_scratch_survives_frame_swaps_and_primitive_kind_changes() {
    use gpui_kit::{Path, PathId, Shadow, Underline, px};
    let mut scene = Scene::default();
    let mut rendered = Scene::default();
    let mut random = 0x5eed_u64;
    for count in [2048, 128, 3, 1024, 0, 65, 2048, 512] {
        std::mem::swap(&mut scene, &mut rendered);
        scene.clear();
        for index in 0..count {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let order = (random % 32) as u32;
            let b = bounds(index as f32, 0., 1., 1.);
            scene.shadows.push(Shadow {
                order,
                blur_radius: ScaledPixels(2.),
                bounds: b,
                corner_radii: Default::default(),
                content_mask: ContentMask { bounds: b },
                color: rgb(index).into(),
                element_bounds: b,
                element_corner_radii: Default::default(),
                inset: 0,
                pad: 0,
            });
            if index % 2 == 0 {
                scene.quads.push(Quad {
                    order,
                    bounds: b,
                    ..Default::default()
                });
            }
            if index % 3 == 0 {
                scene.underlines.push(Underline {
                    order,
                    pad: 0,
                    bounds: b,
                    content_mask: ContentMask { bounds: b },
                    color: rgb(index).into(),
                    thickness: ScaledPixels(1.),
                    wavy: (index % 2 == 0).into(),
                });
            }
            if index % 4 == 0 {
                let mut path = Path::new(point(px(index as f32), px(0.)));
                path.id = PathId(index as usize);
                path.order = order;
                path.push_triangle(
                    (
                        point(px(0.), px(0.)),
                        point(px(1.), px(0.)),
                        point(px(0.), px(1.)),
                    ),
                    (point(0., 0.), point(1., 0.), point(0., 1.)),
                );
                scene.paths.push(path.scale(1.));
            }
        }
        let mut shadows = scene.shadows.clone();
        let mut quads = scene.quads.clone();
        let mut underlines = scene.underlines.clone();
        let mut paths = scene.paths.clone();
        shadows.sort_by_key(|p| p.order);
        quads.sort_by_key(|p| p.order);
        underlines.sort_by_key(|p| p.order);
        paths.sort_by_key(|p| p.order);
        scene.finish();
        assert_eq!(format!("{:?}", scene.shadows), format!("{shadows:?}"));
        assert_eq!(format!("{:?}", scene.quads), format!("{quads:?}"));
        assert_eq!(format!("{:?}", scene.underlines), format!("{underlines:?}"));
        assert_eq!(format!("{:?}", scene.paths), format!("{paths:?}"));
    }
}

#[test]
fn gpui_fast_reverse_equal_keys_keep_insertion_order() {
    for orders in [
        vec![u32::MAX, u32::MAX, 8, 8, 1, 0, 0],
        vec![7; 32],
        vec![9, 5, 1],
    ] {
        let mut scene = Scene::default();
        scene.quads = orders
            .into_iter()
            .enumerate()
            .map(|(index, order)| Quad {
                order,
                bounds: bounds(index as f32, 0., 1., 1.),
                ..Default::default()
            })
            .collect();
        let mut expected = scene.quads.clone();
        expected.sort_by_key(|quad| quad.order);
        scene.finish();
        assert_eq!(format!("{:?}", scene.quads), format!("{expected:?}"));
    }
}

#[test]
fn gpui_fast_permutation_cycles_cover_every_six_item_order() {
    fn visit(orders: &mut [u32], start: usize, scene: &mut Scene) {
        if start == orders.len() {
            scene.clear();
            scene.quads = orders
                .iter()
                .map(|&order| Quad {
                    order,
                    ..Default::default()
                })
                .collect();
            scene.finish();
            assert_eq!(
                scene
                    .quads
                    .iter()
                    .map(|quad| quad.order)
                    .collect::<Vec<_>>(),
                vec![0, 1, 2, 3, 4, 5]
            );
            return;
        }
        for next in start..orders.len() {
            orders.swap(start, next);
            visit(orders, start + 1, scene);
            orders.swap(start, next);
        }
    }
    visit(&mut [0, 1, 2, 3, 4, 5], 0, &mut Scene::default());
}

#[test]
fn gpui_fast_sprite_keys_preserve_all_bits_and_equal_key_order() {
    use gpui_kit::{
        AtlasTextureId, AtlasTextureKind, AtlasTile, DevicePixels, MonochromeSprite, TileId,
        TransformationMatrix,
    };
    let mut scene = Scene::default();
    let mut random = 0x5eed_u64;
    for ordered in [false, true] {
        scene.clear();
        for index in 0..1024 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let (order, texture, tile) = if ordered {
                (index / 32, index % 4, index / 4)
            } else if index % 3 == 0 {
                (u32::MAX, u32::MAX, u32::MAX)
            } else {
                (random as u32, (random >> 16) as u32, (random >> 32) as u32)
            };
            let b = bounds(index as f32, 0., 1., 1.);
            scene.monochrome_sprites.push(MonochromeSprite {
                order,
                pad: 0,
                bounds: b,
                content_mask: ContentMask { bounds: b },
                color: rgb(index).into(),
                transformation: TransformationMatrix::unit(),
                tile: AtlasTile {
                    texture_id: AtlasTextureId {
                        index: texture,
                        kind: AtlasTextureKind::Monochrome,
                    },
                    tile_id: TileId(tile),
                    padding: 0,
                    bounds: Bounds::new(
                        point(DevicePixels(0), DevicePixels(0)),
                        size(DevicePixels(1), DevicePixels(1)),
                    ),
                },
            });
        }
        let mut expected = scene.monochrome_sprites.clone();
        if !ordered {
            expected.sort_by_key(|sprite| {
                (
                    sprite.order,
                    sprite.tile.texture_id.index,
                    sprite.tile.tile_id,
                )
            });
        }
        for _ in 0..2 {
            scene.finish();
            assert_eq!(
                format!("{:?}", scene.monochrome_sprites),
                format!("{expected:?}")
            );
        }
    }
}

#[test]
fn gpui_fast_atlas_grouping_preserves_overlapping_paint_order() {
    use gpui_kit::{
        AtlasTextureId, AtlasTextureKind, AtlasTile, DevicePixels, MonochromeSprite,
        PolychromeSprite, PrimitiveBatch, SubpixelSprite, TileId, TransformationMatrix,
    };

    for kind in [
        AtlasTextureKind::Monochrome,
        AtlasTextureKind::Subpixel,
        AtlasTextureKind::Polychrome,
    ] {
        let mut scene = Scene::default();
        let insert = |scene: &mut Scene, index: u32| {
            let bounds = bounds(index as f32 * 12., 0., 10., 10.);
            let content_mask = ContentMask { bounds };
            let tile = AtlasTile {
                texture_id: AtlasTextureId {
                    index: index % 2,
                    kind,
                },
                tile_id: TileId(index / 2),
                padding: 0,
                bounds: Bounds::new(
                    point(DevicePixels(0), DevicePixels(0)),
                    size(DevicePixels(10), DevicePixels(10)),
                ),
            };
            match kind {
                AtlasTextureKind::Monochrome => scene.insert_primitive(MonochromeSprite {
                    order: 0,
                    pad: 0,
                    bounds,
                    content_mask,
                    tile,
                    color: rgb(0xffffff).into(),
                    transformation: TransformationMatrix::unit(),
                }),
                AtlasTextureKind::Subpixel => scene.insert_primitive(SubpixelSprite {
                    order: 0,
                    pad: 0,
                    bounds,
                    content_mask,
                    tile,
                    color: rgb(0xffffff).into(),
                    transformation: TransformationMatrix::unit(),
                }),
                AtlasTextureKind::Polychrome => scene.insert_primitive(PolychromeSprite {
                    order: 0,
                    pad: 0,
                    bounds,
                    content_mask,
                    tile,
                    grayscale: false.into(),
                    opacity: 1.,
                    corner_radii: Default::default(),
                }),
            }
        };
        for index in [6, 1, 4, 3, 2, 5, 0, 7] {
            insert(&mut scene, index);
        }
        let overlay = bounds(0., 0., 100., 10.);
        scene.insert_primitive(Quad {
            bounds: overlay,
            content_mask: ContentMask { bounds: overlay },
            background: rgb(0xff0000).into(),
            ..Default::default()
        });
        insert(&mut scene, 0);
        scene.finish();
        let batches: Vec<_> = scene.batches().collect();
        assert_eq!(
            batches.len(),
            4,
            "two atlas batches, overlay, foreground glyph"
        );
        assert!(matches!(&batches[2], PrimitiveBatch::Quads(range) if *range == (0..1)));
        let sprites: Vec<_> = match kind {
            AtlasTextureKind::Monochrome => scene
                .monochrome_sprites
                .iter()
                .map(|p| (p.order, p.tile))
                .collect(),
            AtlasTextureKind::Subpixel => scene
                .subpixel_sprites
                .iter()
                .map(|p| (p.order, p.tile))
                .collect(),
            AtlasTextureKind::Polychrome => scene
                .polychrome_sprites
                .iter()
                .map(|p| (p.order, p.tile))
                .collect(),
        };
        for (index, (order, tile)) in sprites[..8].iter().enumerate() {
            assert!(*order < scene.quads[0].order);
            assert_eq!(tile.texture_id.index, (index / 4) as u32);
        }
        assert!(sprites[8].0 > scene.quads[0].order);
    }
}
