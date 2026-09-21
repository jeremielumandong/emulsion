//! Enforcement for independent layer locks, shared by UI and scripting commands.
use crate::{Command, CommandError, Document, NodeKind};
use emulsion_raster::{Raster, TILE, TILE_PX};
use std::collections::HashSet;

pub(crate) fn check(command: &Command, doc: &Document) -> Result<(), CommandError> {
    let (id, pixels, position, changes_alpha) = match command {
        Command::SetFillColor { id, rgba } => {
            let alpha_changed = doc.node(*id).is_some_and(
                |node| matches!(node.kind, NodeKind::Fill { rgba: old } if old[3] != rgba[3]),
            );
            (*id, true, false, alpha_changed)
        }
        Command::ReplacePixels { id, raster, .. } => {
            if doc.layer_locks(*id).transparency
                && let Some(crate::Node {
                    kind: NodeKind::Raster { raster: old, .. },
                    ..
                }) = doc.node(*id)
                && (old.width(), old.height()) != (raster.width(), raster.height())
            {
                return Err(CommandError::Locked(*id));
            }
            (*id, true, false, false)
        }
        Command::ApplyLayerMask { id } => (*id, true, false, true),
        Command::SetMaskTransform { id, .. } | Command::SetMaskLinked { id, .. } => {
            (*id, false, true, false)
        }
        Command::ReplaceContent { id, .. } => (*id, true, true, true),
        Command::SetPlacement { id, .. }
        | Command::RotateNode { id, .. }
        | Command::TranslateNode { id, .. }
        | Command::AlignNode { id, .. } => (*id, false, true, false),
        Command::SetFilters { id, .. }
        | Command::SetSmartCache { id, .. }
        | Command::ConvertToLayers { id } => (*id, true, false, true),
        Command::Rasterize { id } => (*id, true, false, false),
        Command::SetText { id, spec } => {
            let Some(crate::Node {
                kind: NodeKind::Text { spec: old, .. },
                ..
            }) = doc.node(*id)
            else {
                return Ok(());
            };
            let position = old.x != spec.x
                || old.y != spec.y
                || old.rotation != spec.rotation
                || old.scale_x != spec.scale_x
                || old.scale_y != spec.scale_y;
            let mut normalized = (**spec).clone();
            normalized.x = old.x;
            normalized.y = old.y;
            normalized.rotation = old.rotation;
            normalized.scale_x = old.scale_x;
            normalized.scale_y = old.scale_y;
            let pixels = normalized != **old;
            normalized.color[..3].copy_from_slice(&old.color[..3]);
            let changes_alpha = normalized != **old;
            (*id, pixels, position, changes_alpha)
        }
        Command::SetPath { id, path, .. } => {
            let moved = doc.node(*id).is_some_and(
                |n| matches!(&n.kind, NodeKind::Path { path: old, .. } if old != path),
            );
            (*id, true, moved, true)
        }
        _ => return Ok(()),
    };
    for node in doc
        .nodes
        .iter()
        .filter(|n| n.id == id || doc.is_ancestor(id, n.id))
    {
        let locks = doc.layer_locks(node.id);
        if (pixels && locks.pixels)
            || (position && locks.position)
            || (changes_alpha && locks.transparency)
        {
            return Err(CommandError::Locked(node.id));
        }
    }
    Ok(())
}

fn locked_pixel(old: [u16; 4], proposed: [u16; 4]) -> [u16; 4] {
    if old[3] == 0 {
        return [0; 4];
    }
    if proposed[3] == 0 {
        return old;
    }
    let mut out = [0; 4];
    for i in 0..3 {
        out[i] = ((u64::from(proposed[i]) * u64::from(old[3]) + u64::from(proposed[3]) / 2)
            / u64::from(proposed[3]))
        .min(u64::from(old[3])) as u16;
    }
    out[3] = old[3];
    out
}

/// Keep the original alpha exactly, recoloring only existing coverage. Tile
/// iteration preserves sparsity rather than walking a potentially huge canvas.
pub(crate) fn preserve_alpha(old: &Raster, proposed: &Raster) -> Raster {
    let mut out = Raster::empty(
        old.width(),
        old.height(),
        locked_pixel(old.fill(), proposed.fill()),
    );
    let coords: HashSet<_> = old
        .base_tiles()
        .chain(proposed.base_tiles())
        .map(|(c, _)| *c)
        .collect();
    for coord in coords {
        let old_tile = old.base_tile(coord);
        let new_tile = proposed.base_tile(coord);
        let mut pixels = Vec::with_capacity(TILE_PX);
        for i in 0..TILE_PX {
            let x = coord.x as i64 * i64::from(TILE) + (i % TILE as usize) as i64;
            let y = coord.y as i64 * i64::from(TILE) + (i / TILE as usize) as i64;
            if x < 0 || y < 0 || x >= i64::from(old.width()) || y >= i64::from(old.height()) {
                pixels.push(out.fill());
            } else {
                pixels.push(locked_pixel(
                    old_tile.map_or(old.fill(), |tile| tile[i]),
                    new_tile.map_or(proposed.fill(), |tile| tile[i]),
                ));
            }
        }
        out.set_tile(coord, pixels);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Slot;
    use crate::node::{LayerColor, LayerLocks};
    use crate::{Node, history::Editor};
    use emulsion_raster::{IRect, Placement};
    use std::sync::Arc;

    fn editor() -> (Editor, u64) {
        let mut e = Editor::new(Document::new(4, 1), None);
        let raster = Raster::from_pixels(
            4,
            1,
            [0; 4],
            &[
                [65535, 0, 0, 65535],
                [0, 16000, 0, 32768],
                [0; 4],
                [12000, 0, 0, 12000],
            ],
        );
        let id = e
            .execute(Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Pixels",
                    Arc::new(raster),
                    Placement::default(),
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        (e, id)
    }

    #[test]
    fn alpha_lock_preserves_partial_coverage_and_blocks_eraser() {
        let (mut e, id) = editor();
        e.execute(Command::SetLayerLocks {
            id,
            locks: LayerLocks {
                transparency: true,
                ..Default::default()
            },
        })
        .unwrap();
        e.execute(Command::ReplacePixels {
            id,
            raster: Arc::new(Raster::from_pixels(
                4,
                1,
                [0; 4],
                &[
                    [0, 0, 65535, 65535],
                    [32768, 0, 0, 65535],
                    [65535; 4],
                    [0; 4],
                ],
            )),
            dirty: IRect::new(0, 0, 4, 1),
            label: "Paint".into(),
        })
        .unwrap();
        let NodeKind::Raster { raster, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(raster.get(0, 0), [0, 0, 65535, 65535]);
        assert_eq!(raster.get(1, 0), [16384, 0, 0, 32768]);
        assert_eq!(raster.get(2, 0), [0; 4]);
        assert_eq!(raster.get(3, 0), [12000, 0, 0, 12000]);
        assert!(e.undo());
        assert!(e.doc.node(id).unwrap().locks.transparency);
    }

    #[test]
    fn pixel_and_position_locks_are_independent_and_inherited() {
        let (mut e, id) = editor();
        e.execute(Command::SetLayerLocks {
            id,
            locks: LayerLocks {
                pixels: true,
                ..Default::default()
            },
        })
        .unwrap();
        let change = Command::ReplacePixels {
            id,
            raster: Arc::new(Raster::transparent(4, 1)),
            dirty: IRect::new(0, 0, 4, 1),
            label: "Clear".into(),
        };
        assert!(matches!(e.execute(change), Err(CommandError::Locked(_))));
        e.execute(Command::TranslateNode {
            id,
            dx: 3.0,
            dy: 0.0,
        })
        .unwrap();
        let group = e
            .execute(Command::Group {
                ids: vec![id],
                name: "Group".into(),
            })
            .unwrap()
            .unwrap();
        e.execute(Command::SetLayerLocks {
            id: group,
            locks: LayerLocks {
                position: true,
                ..Default::default()
            },
        })
        .unwrap();
        assert!(matches!(
            e.execute(Command::TranslateNode {
                id,
                dx: 1.0,
                dy: 0.0
            }),
            Err(CommandError::Locked(_))
        ));
        assert!(matches!(
            e.execute(Command::RotateNode {
                id: group,
                degrees: 90.0
            }),
            Err(CommandError::Locked(_))
        ));
        e.execute(Command::SetColorLabel {
            id,
            color: LayerColor::Blue,
        })
        .unwrap();
        assert_eq!(e.doc.node(id).unwrap().color_label, LayerColor::Blue);
        assert!(e.undo());
        assert_eq!(e.doc.node(id).unwrap().color_label, LayerColor::None);
    }

    #[test]
    fn multiple_move_is_atomic_and_selected_descendants_move_once() {
        let (mut e, id) = editor();
        let other = e
            .execute(Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Other",
                    Arc::new(Raster::transparent(4, 1)),
                    Placement::default(),
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        e.execute(Command::SetLayerLocks {
            id: other,
            locks: LayerLocks {
                position: true,
                ..Default::default()
            },
        })
        .unwrap();
        let before = e.doc.clone();
        assert!(
            e.execute(Command::TranslateNodes {
                ids: vec![id, other],
                dx: 5.0,
                dy: 3.0
            })
            .is_err()
        );
        assert_eq!(e.doc, before);
        let group = e
            .execute(Command::Group {
                ids: vec![id],
                name: "Group".into(),
            })
            .unwrap()
            .unwrap();
        e.execute(Command::TranslateNodes {
            ids: vec![group, id, id],
            dx: 5.0,
            dy: 3.0,
        })
        .unwrap();
        let NodeKind::Raster { placement, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!((placement.x, placement.y), (5.0, 3.0));
        assert!(e.undo());
        let NodeKind::Raster { placement, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!((placement.x, placement.y), (0.0, 0.0));
    }

    #[test]
    fn locked_text_mixed_payload_and_smart_conversion_cannot_bypass_locks() {
        use crate::text::TextSpec;
        let mut e = Editor::new(Document::new(100, 60), None);
        let original = TextSpec {
            text: "Text".into(),
            size: 20.0,
            ..Default::default()
        };
        let id = e
            .execute(Command::AddNode {
                node: Box::new(Node::text(0, "Type", original.clone(), 100, 60)),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        e.execute(Command::SetLayerLocks {
            id,
            locks: LayerLocks {
                position: true,
                ..Default::default()
            },
        })
        .unwrap();
        let before = e.doc.clone();
        let mut mixed = original.clone();
        mixed.x += 20.0;
        mixed.text = "Changed".into();
        assert!(
            e.execute(Command::SetText {
                id,
                spec: Box::new(mixed)
            })
            .is_err()
        );
        assert_eq!(e.doc, before, "mixed edits fail atomically");
        e.execute(Command::SetLayerLocks {
            id,
            locks: LayerLocks {
                transparency: true,
                ..Default::default()
            },
        })
        .unwrap();
        let mut recolored = original.clone();
        recolored.color = [200, 30, 10, 255];
        e.execute(Command::SetText {
            id,
            spec: Box::new(recolored.clone()),
        })
        .unwrap();
        recolored.color[3] = 128;
        assert!(
            e.execute(Command::SetText {
                id,
                spec: Box::new(recolored)
            })
            .is_err()
        );
        e.execute(Command::SetLayerLocks {
            id,
            locks: LayerLocks {
                pixels: true,
                ..Default::default()
            },
        })
        .unwrap();
        assert!(e.execute(Command::Rasterize { id }).is_err());
        e.execute(Command::ConvertToSmart { id }).unwrap();
        let smart = e.doc.clone();
        assert!(e.execute(Command::ConvertToLayers { id }).is_err());
        assert!(
            e.execute(Command::SetFilters {
                id,
                filters: Vec::new()
            })
            .is_err()
        );
        assert_eq!(e.doc, smart);
    }

    #[test]
    fn image_pixel_lock_does_not_prevent_independent_mask_editing() {
        let (mut e, id) = editor();
        e.execute(Command::SetLayerLocks {
            id,
            locks: LayerLocks {
                pixels: true,
                transparency: true,
                ..Default::default()
            },
        })
        .unwrap();
        let mask = Arc::new(emulsion_raster::Mask::empty(4, 1, 128));
        e.execute(Command::SetMask {
            id,
            mask: Some(mask.clone()),
        })
        .unwrap();
        assert!(Arc::ptr_eq(
            e.doc.node(id).unwrap().mask.as_ref().unwrap(),
            &mask
        ));
    }

    #[test]
    fn fill_color_preserves_mask_identity_styles_and_undo() {
        let mut e = Editor::new(Document::new(20, 20), None);
        let mut node = Node::new(
            0,
            "Ellipse",
            NodeKind::Fill {
                rgba: [0, 0, 255, 255],
            },
        );
        let mask = Arc::new(emulsion_raster::Mask::from_fn(20, 20, 0, |x, y| {
            if (5..15).contains(&x) && (5..15).contains(&y) {
                255
            } else {
                0
            }
        }));
        node.mask = Some(mask.clone());
        node.styles = vec![crate::styles::LayerStyle::catalogue()[0].clone()];
        let id = e
            .execute(Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let original = e.doc.node(id).unwrap().clone();
        e.execute(Command::SetFillColor {
            id,
            rgba: [255, 30, 10, 255],
        })
        .unwrap();
        assert_eq!(e.doc.nodes.len(), 1);
        let changed = e.doc.node(id).unwrap();
        assert!(matches!(
            changed.kind,
            NodeKind::Fill {
                rgba: [255, 30, 10, 255]
            }
        ));
        assert!(Arc::ptr_eq(changed.mask.as_ref().unwrap(), &mask));
        assert_eq!(changed.styles, original.styles);
        assert_eq!(changed.name, original.name);
        let rendered = emulsion_raster::composite::flatten(&e.doc.composite_tree(), 0);
        assert_eq!(
            rendered.get(0, 0),
            [0; 4],
            "recoloring retains transparent masked surroundings"
        );
        assert_eq!(rendered.get(10, 10)[0], 65535);
        assert!(e.undo());
        assert_eq!(e.doc.node(id).unwrap(), &original);
        assert!(e.redo());
        assert!(matches!(
            e.doc.node(id).unwrap().kind,
            NodeKind::Fill {
                rgba: [255, 30, 10, 255]
            }
        ));
    }

    #[test]
    fn fill_color_respects_pixel_alpha_and_full_locks() {
        let mut e = Editor::new(Document::new(4, 4), None);
        let id = e
            .execute(Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    "Fill",
                    NodeKind::Fill {
                        rgba: [0, 0, 0, 255],
                    },
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        e.execute(Command::SetLayerLocks {
            id,
            locks: LayerLocks {
                transparency: true,
                ..Default::default()
            },
        })
        .unwrap();
        e.execute(Command::SetFillColor {
            id,
            rgba: [10, 20, 30, 255],
        })
        .unwrap();
        assert!(
            e.execute(Command::SetFillColor {
                id,
                rgba: [10, 20, 30, 128]
            })
            .is_err()
        );
        e.execute(Command::SetLayerLocks {
            id,
            locks: LayerLocks {
                pixels: true,
                ..Default::default()
            },
        })
        .unwrap();
        assert!(
            e.execute(Command::SetFillColor { id, rgba: [255; 4] })
                .is_err()
        );
        e.execute(Command::SetLayerLocks {
            id,
            locks: LayerLocks::default(),
        })
        .unwrap();
        e.execute(Command::SetLocked { id, locked: true }).unwrap();
        assert!(
            e.execute(Command::SetFillColor { id, rgba: [255; 4] })
                .is_err()
        );
        assert!(matches!(
            e.doc.node(id).unwrap().kind,
            NodeKind::Fill {
                rgba: [10, 20, 30, 255]
            }
        ));
    }

    #[test]
    fn rasterize_fill_keeps_mask_appearance_identity_and_undo() {
        let mut e = Editor::new(Document::new(12, 8), None);
        let mask = Arc::new(emulsion_raster::Mask::from_fn(12, 8, 0, |x, _| {
            if x < 6 { 128 } else { 0 }
        }));
        let mut node = Node::new(
            0,
            "Masked fill",
            NodeKind::Fill {
                rgba: [200, 50, 25, 180],
            },
        );
        node.mask = Some(mask.clone());
        let id = e
            .execute(Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let original = e.doc.node(id).unwrap().clone();
        let before = emulsion_raster::composite::flatten(&e.doc.composite_tree(), 0).to_srgba8();
        e.execute(Command::Rasterize { id }).unwrap();
        assert_eq!(e.doc.nodes.len(), 1);
        let rasterized = e.doc.node(id).unwrap();
        assert_eq!(rasterized.name, original.name);
        assert_eq!(rasterized.styles, original.styles);
        assert_eq!(rasterized.mask_enabled, original.mask_enabled);
        assert!(Arc::ptr_eq(rasterized.mask.as_ref().unwrap(), &mask));
        let NodeKind::Raster { raster, placement } = &rasterized.kind else {
            panic!("editable pixel layer")
        };
        assert_eq!((raster.width(), raster.height()), (12, 8));
        assert_eq!(*placement, Placement::default());
        assert_eq!(
            emulsion_raster::composite::flatten(&e.doc.composite_tree(), 0).to_srgba8(),
            before
        );
        assert!(e.undo());
        assert_eq!(e.doc.node(id).unwrap(), &original);
    }
}
