//! Storyboard editing that changes pages and storyboard data together, each
//! as one project Undo step: inserting, Smart add, moving panels between
//! scenes, renumbering, converting thumbnail sheets and the panel clipboard.
use super::{MAX_PAGES, MAX_PROJECT_PIXELS, PageId, PageMeta, ProjectEditor};
use crate::Document;
use crate::command::Slot;
use crate::storyboard::{
    Caption, CaptionField, FrameRate, Level, MAX_PANEL_FRAMES, Panel, RenumberScope, Storyboard,
};
use crate::storyboard_naming::{centred_frame, fit_document};
use crate::{Editor, fragment::Fragment};
use std::collections::HashSet;
use std::sync::Arc;

/// Where a new group starts within a batch of inserted panels.
pub struct GroupStart {
    /// Index into the inserted panels.
    pub at: usize,
    pub level: Level,
    pub name: Option<String>,
}

/// Panels copied from a storyboard, ready to paste into any storyboard.
#[derive(Clone, Debug)]
pub struct PanelClip {
    pub frame_rate: FrameRate,
    pub fields: Vec<CaptionField>,
    /// Scene names in clip order.
    pub scenes: Vec<String>,
    /// Every copied scene was copied whole, so pasting recreates the scenes.
    pub whole_scenes: bool,
    pub panels: Vec<ClipPanel>,
}

#[derive(Clone, Debug)]
pub struct ClipPanel {
    pub name: String,
    pub doc: Document,
    /// Panel data; `scene` is an index into `PanelClip::scenes`.
    pub panel: Panel,
}

impl ProjectEditor {
    /// Match every page editor's lock to the storyboard.
    pub(super) fn refresh_locks(&mut self) {
        let board = self.storyboard.clone();
        for (id, editor) in &mut self.pages {
            editor.set_read_only(board.as_ref().is_some_and(|b| b.is_locked(*id)));
        }
    }

    pub(super) fn board(&self) -> Result<&Arc<Storyboard>, String> {
        self.storyboard
            .as_ref()
            .ok_or_else(|| "This is not a storyboard project.".into())
    }

    fn layout_ids(&self) -> Vec<PageId> {
        self.layout.iter().map(|m| m.id).collect()
    }

    /// Insert panels with their documents after `after` (first when `None`)
    /// as one Undo step, optionally replacing the page `replace`. A panel
    /// whose `scene` exists keeps it; any other joins the scene before it.
    /// The first new panel becomes active.
    pub fn insert_panel_documents(
        &mut self,
        after: Option<PageId>,
        items: Vec<(String, Document, Panel)>,
        starts: &[GroupStart],
        replace: Option<PageId>,
    ) -> Result<Vec<PageId>, String> {
        let base = Storyboard::clone(self.board()?);
        self.insert_into(base, after, items, starts, replace)
    }

    /// `insert_panel_documents` starting from `base`, the storyboard with any
    /// fields or scenes the new panels need.
    fn insert_into(
        &mut self,
        mut next: Storyboard,
        after: Option<PageId>,
        items: Vec<(String, Document, Panel)>,
        starts: &[GroupStart],
        replace: Option<PageId>,
    ) -> Result<Vec<PageId>, String> {
        let board = &next;
        let removing = usize::from(replace.is_some());
        if items.is_empty() || self.layout.len() + items.len() - removing > MAX_PAGES {
            return Err(format!("A project supports 1–{MAX_PAGES} pages."));
        }
        if let Some(page) = replace {
            if !self.layout.iter().any(|m| m.id == page) {
                return Err("Panel does not exist.".into());
            }
            if board.is_locked(page) {
                return Err("That panel is locked. Unlock it first.".into());
            }
        }
        for (_, doc, _) in &items {
            doc.validate().map_err(|e| e.to_string())?;
            self.check_panel_size(doc)?;
        }
        let area = u64::from(board.settings.width) * u64::from(board.settings.height);
        if (self.layout.len() + items.len() - removing) as u64 * area > MAX_PROJECT_PIXELS {
            return Err("Project exceeds the total page area limit.".into());
        }
        let count = items.len() as u64;
        if self
            .next_page_id
            .checked_add(count)
            .is_none_or(|id| id >= u64::MAX - 1)
        {
            return Err("Page ID limit reached.".into());
        }
        if starts.iter().any(|s| s.at >= items.len()) {
            return Err("A group start is outside the inserted panels.".into());
        }
        let index = match after {
            Some(after) => {
                self.layout
                    .iter()
                    .position(|m| m.id == after)
                    .ok_or("Panel does not exist.")?
                    + 1
            }
            None => 0,
        };
        let mut layout = self.layout.clone();
        let mut ids = Vec::new();
        for (offset, (name, _, _)) in items.iter().enumerate() {
            let meta = PageMeta {
                id: self.next_page_id + offset as u64,
                name: name.trim().into(),
                bleed_mm: 0.,
            };
            meta.validate()?;
            ids.push(meta.id);
            layout.insert(index + offset, meta);
        }
        layout.retain(|m| Some(m.id) != replace);
        let order: Vec<_> = layout.iter().map(|m| m.id).collect();
        let mut docs = Vec::new();
        let mut assign = Vec::new();
        for (id, (_, doc, panel)) in ids.iter().zip(items) {
            docs.push(doc);
            if next.scenes.contains_key(&panel.scene) {
                next.panels.insert(*id, panel);
            } else {
                assign.push((*id, panel));
            }
        }
        next.reconcile(&order);
        for (id, panel) in assign {
            let scene = next.panels[&id].scene;
            next.panels.insert(id, Panel { scene, ..panel });
        }
        for start in starts {
            next.split(&order, ids[start.at], start.level, start.name.as_deref())?;
        }
        next.validate(&order)?;
        self.board()?.check_locks_kept(&next)?;
        self.record_pages()?;
        for (id, doc) in ids.iter().zip(docs) {
            self.pages.insert(*id, Editor::new(doc, self.path.clone()));
        }
        self.layout = layout;
        self.next_page_id += count;
        self.active = ids[0];
        self.storyboard = Some(Arc::new(next));
        self.collect_pages();
        self.refresh_locks();
        Ok(ids)
    }

    /// Smart add: a new panel after `after`, in its scene, holding copies of
    /// the source panel's top-level layers named in the Smart add list
    /// (replacing the blank panel's layer of the same name).
    pub fn smart_add_panel(&mut self, after: PageId) -> Result<PageId, String> {
        let board = self.board()?;
        let source = self.page(after).ok_or("Panel does not exist.")?;
        let wanted: HashSet<_> = board
            .smart_add_layers
            .iter()
            .map(|n| n.trim().to_lowercase())
            .collect();
        let carried: Vec<_> = source
            .doc
            .nodes
            .iter()
            .filter(|n| n.parent.is_none() && wanted.contains(&n.name.trim().to_lowercase()))
            .map(|n| n.id)
            .collect();
        let mut panel = Panel::new(board.panels[&after].scene, board.settings.panel_frames);
        panel.size = board.panels[&after].size;
        panel.angle = board.panels[&after].angle;
        let mut editor = Editor::new(board.blank_panel()?, None);
        if !carried.is_empty() {
            let names: HashSet<_> = carried
                .iter()
                .map(|id| source.doc.node(*id).unwrap().name.trim().to_lowercase())
                .collect();
            let replaced: Vec<_> = editor
                .doc
                .nodes
                .iter()
                .filter(|n| n.parent.is_none() && names.contains(&n.name.trim().to_lowercase()))
                .map(|n| n.id)
                .collect();
            for id in replaced {
                editor
                    .execute(crate::Command::RemoveNode { id })
                    .map_err(|e| e.to_string())?;
            }
            Fragment::capture(&source.doc, &carried)?.paste(&mut editor, Slot::TOP, (0., 0.))?;
        }
        let name = self.next_panel_name(after);
        let ids =
            self.insert_panel_documents(Some(after), vec![(name, editor.doc, panel)], &[], None)?;
        Ok(ids[0])
    }

    /// A panel name for a panel inserted after `after`, by the naming rules.
    fn next_panel_name(&self, after: PageId) -> String {
        let Some(board) = &self.storyboard else {
            return "Panel".into();
        };
        let scene = board.panels.get(&after).map(|p| p.scene);
        let count = if board.naming.panels_per_scene {
            board
                .panels
                .values()
                .filter(|p| Some(p.scene) == scene)
                .count()
        } else {
            board.panels.len()
        };
        board.naming.panel_name(count + 1)
    }

    /// Move panels, in their page order, to `to` (a position in the layout
    /// without them). With `scene`, they join that scene, which must be next
    /// to where they land; otherwise they join the scene they land in.
    pub fn move_panels(
        &mut self,
        ids: &[PageId],
        to: usize,
        scene: Option<crate::storyboard::GroupId>,
    ) -> Result<(), String> {
        let board = self.board()?;
        let moving: HashSet<_> = ids.iter().copied().collect();
        if moving.is_empty() || moving.len() != ids.len() {
            return Err("Choose each panel to move once.".into());
        }
        if ids
            .iter()
            .any(|id| !self.layout.iter().any(|m| m.id == *id))
        {
            return Err("Panel does not exist.".into());
        }
        if scene.is_some_and(|scene| !board.scenes.contains_key(&scene)) {
            return Err("No scene has that ID.".into());
        }
        let (moved, mut layout): (Vec<_>, Vec<_>) = self
            .layout
            .iter()
            .cloned()
            .partition(|m| moving.contains(&m.id));
        if to > layout.len() {
            return Err("Panel position is outside the project.".into());
        }
        layout.splice(to..to, moved);
        let order: Vec<_> = layout.iter().map(|m| m.id).collect();
        let mut next = Storyboard::clone(board);
        match scene {
            Some(scene) => {
                for id in ids {
                    next.panels.get_mut(id).unwrap().scene = scene;
                }
                next.reconcile(&order);
                // Regrouping must not pull in, or push out, anything else.
                if next.panels.iter().any(|(id, p)| {
                    if moving.contains(id) {
                        p.scene != scene
                    } else {
                        p.scene != board.panels[id].scene
                    }
                }) {
                    return Err("Drop the panels inside or next to that scene.".into());
                }
            }
            None => next.reconcile(&order),
        }
        next.validate(&order)?;
        board.check_locks_kept(&next)?;
        if layout == self.layout && next == **board {
            return Ok(());
        }
        self.record_pages()?;
        self.layout = layout;
        self.storyboard = Some(Arc::new(next));
        self.collect_pages();
        self.refresh_locks();
        Ok(())
    }

    /// Rename scenes and/or panels by the naming rules, as one Undo step.
    /// Returns how many names changed.
    pub fn renumber(
        &mut self,
        scope: &RenumberScope,
        scenes: bool,
        panels: bool,
    ) -> Result<usize, String> {
        let board = self.board()?;
        if !scenes && !panels {
            return Err("Choose scenes, panels or both to renumber.".into());
        }
        let order = self.layout_ids();
        let mut next = Storyboard::clone(board);
        let names = next.renumber(&order, scope, scenes, panels)?;
        let mut layout = self.layout.clone();
        let mut changed = next
            .scenes
            .iter()
            .filter(|(id, s)| board.scenes[id].name != s.name)
            .count();
        for meta in &mut layout {
            if let Some(name) = names.get(&meta.id).filter(|name| meta.name != **name) {
                meta.name = name.clone();
                meta.validate()?;
                changed += 1;
            }
        }
        if changed == 0 {
            return Ok(0);
        }
        next.validate(&order)?;
        self.record_pages()?;
        self.layout = layout;
        self.storyboard = Some(Arc::new(next));
        Ok(changed)
    }

    /// Turn a thumbnail sheet into one panel per cell, in row order, in the
    /// sheet's scene and place, each cropped to its camera frame and scaled to
    /// the project resolution. The sheet is removed. One Undo step.
    pub fn convert_thumbnails(&mut self, sheet: PageId) -> Result<Vec<PageId>, String> {
        let board = self.board()?;
        let panel = board.panels.get(&sheet).ok_or("Panel does not exist.")?;
        let grid = panel
            .thumbnails
            .ok_or("That panel is not a thumbnail sheet.")?;
        let (width, height) = (board.settings.width, board.settings.height);
        let doc = &self.page(sheet).ok_or("Panel does not exist.")?.doc;
        let base = Panel {
            thumbnails: None,
            locked: false,
            ..Panel::new(panel.scene, board.settings.panel_frames)
        };
        let after = self
            .layout
            .iter()
            .position(|m| m.id == sheet)
            .and_then(|at| at.checked_sub(1))
            .map(|at| self.layout[at].id);
        let scene_count = board
            .panels
            .values()
            .filter(|p| p.scene == panel.scene)
            .count();
        let items: Vec<_> = grid
            .cells(width, height)
            .into_iter()
            .enumerate()
            .map(|(index, cell)| {
                let number = if board.naming.panels_per_scene {
                    scene_count + index
                } else {
                    board.panels.len() + index
                };
                (
                    board.naming.panel_name(number),
                    fit_document(doc, cell, width, height),
                    base.clone(),
                )
            })
            .collect();
        self.insert_panel_documents(after, items, &[], Some(sheet))
    }

    /// Copy panels, in page order, for `paste_panels` here or in another
    /// storyboard.
    pub fn copy_panels(&self, ids: &[PageId]) -> Result<PanelClip, String> {
        let board = self.board()?;
        let wanted: HashSet<_> = ids.iter().copied().collect();
        if wanted.is_empty() {
            return Err("Choose panels to copy.".into());
        }
        if ids.iter().any(|id| self.page(*id).is_none()) {
            return Err("Panel does not exist.".into());
        }
        let mut clip = PanelClip {
            frame_rate: board.settings.frame_rate,
            fields: board.captions.clone(),
            scenes: Vec::new(),
            whole_scenes: true,
            panels: Vec::new(),
        };
        for scene in board.outline(&self.layout_ids()) {
            let chosen: Vec<_> = scene
                .panels
                .iter()
                .filter(|id| wanted.contains(id))
                .collect();
            if chosen.is_empty() {
                continue;
            }
            clip.whole_scenes &= chosen.len() == scene.panels.len();
            clip.scenes.push(board.scenes[&scene.scene].name.clone());
            for id in chosen {
                let meta = self.layout.iter().find(|m| m.id == *id).unwrap();
                clip.panels.push(ClipPanel {
                    name: meta.name.clone(),
                    doc: self.pages[id].doc.clone(),
                    panel: Panel {
                        scene: clip.scenes.len() as u64 - 1,
                        locked: false,
                        ..board.panels[id].clone()
                    },
                });
            }
        }
        Ok(clip)
    }

    /// Paste copied panels after `after` (first when `None`) as one Undo step.
    /// Whole scenes come back as new scenes, after the scene `after` is in;
    /// other panels join the scene they land in. Captions follow their field
    /// names, adding missing fields the panels use; durations keep their time at this frame
    /// rate; other resolutions are cropped to the centre and scaled.
    pub fn paste_panels(
        &mut self,
        after: Option<PageId>,
        clip: &PanelClip,
    ) -> Result<Vec<PageId>, String> {
        let board = self.board()?;
        if clip.panels.is_empty() {
            return Err("Nothing to paste.".into());
        }
        let order = self.layout_ids();
        if after.is_some_and(|id| !order.contains(&id)) {
            return Err("Panel does not exist.".into());
        }
        let (width, height) = (board.settings.width, board.settings.height);
        let rate = board.settings.frame_rate;
        let mut next = Storyboard::clone(board);
        let used: HashSet<_> = clip
            .panels
            .iter()
            .flat_map(|item| item.panel.captions.keys())
            .collect();
        let mut fields = std::collections::HashMap::new();
        for field in clip.fields.iter().filter(|f| used.contains(&f.id)) {
            let id = match next.caption(&field.name) {
                Some(id) => id,
                None => next.add_caption_field(&field.name, field.multiline, field.print)?,
            };
            fields.insert(field.id, id);
        }
        let outline = next.outline(&order);
        let (after, scenes) = if clip.whole_scenes {
            // Land on a scene boundary and give each copied scene a new scene
            // in the sequence there.
            let landing = after.and_then(|id| outline.iter().find(|s| s.panels.contains(&id)));
            let sequence = landing.or(outline.first()).map(|s| s.sequence).unwrap();
            let scenes: Vec<_> = clip
                .scenes
                .iter()
                .map(|name| {
                    let id = next.add_scene(sequence);
                    if crate::storyboard::check_name(name, "Scene").is_ok() {
                        next.scenes.get_mut(&id).unwrap().name = name.clone();
                    }
                    id
                })
                .collect();
            (landing.map(|s| *s.panels.last().unwrap()), scenes)
        } else {
            (after, Vec::new())
        };
        let items = clip
            .panels
            .iter()
            .map(|item| {
                let seconds = f64::from(item.panel.frames) / clip.frame_rate.fps();
                let frames = ((seconds * rate.fps()).round() as u32).clamp(1, MAX_PANEL_FRAMES);
                let captions = item
                    .panel
                    .captions
                    .iter()
                    .filter_map(|(id, text)| Some((*fields.get(id)?, Caption::clone(text))))
                    .collect();
                let doc = if (item.doc.width, item.doc.height) == (width, height) {
                    item.doc.clone()
                } else {
                    fit_document(
                        &item.doc,
                        centred_frame(&item.doc, width, height),
                        width,
                        height,
                    )
                };
                let scene = scenes.get(item.panel.scene as usize).copied().unwrap_or(0);
                let panel = Panel {
                    scene,
                    frames,
                    captions,
                    ..item.panel.clone()
                };
                (item.name.clone(), doc, panel)
            })
            .collect();
        self.insert_into(next, after, items, &[], None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectKind;
    use crate::storyboard::{FindOptions, Naming, ThumbnailGrid};
    use crate::{Command, Node, NodeKind};

    fn board(panels: usize) -> ProjectEditor {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        if panels > 1 {
            let blank = p.storyboard().unwrap().blank_panel().unwrap();
            let items = (2..=panels)
                .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
                .collect();
            p.insert_panels(Some(1), &blank, items, None).unwrap();
        }
        p
    }
    fn layout(p: &ProjectEditor) -> Vec<PageId> {
        p.page_list().iter().map(|m| m.id).collect()
    }
    fn layer(p: &mut ProjectEditor, name: &str) {
        p.execute(Command::AddNode {
            node: Box::new(Node::new(0, name, NodeKind::Fill { rgba: [200; 4] })),
            slot: Slot::TOP,
        })
        .unwrap();
    }
    fn valid(p: &ProjectEditor) {
        p.snapshot().unwrap().validate().unwrap();
    }

    #[test]
    fn locked_panels_refuse_drawing_data_changes_and_removal() {
        let mut p = board(2);
        let [first, second] = layout(&p)[..] else {
            unreachable!()
        };
        p.edit_storyboard(|b| {
            b.panels.get_mut(&first).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        p.set_active_page(first).unwrap();
        assert!(p.is_read_only());
        let refused = p.execute(Command::AddNode {
            node: Box::new(Node::new(0, "Hero", NodeKind::Fill { rgba: [1; 4] })),
            slot: Slot::TOP,
        });
        assert!(matches!(refused, Err(crate::CommandError::ReadOnly)));
        assert!(p.remove_page(first).is_err());
        assert!(
            p.edit_storyboard(|b| {
                b.panels.get_mut(&first).unwrap().frames = 3;
                Ok(())
            })
            .is_err()
        );
        // Grouping can still change, and the other panel is free.
        p.edit_storyboard(|b| {
            b.split(&[first, second], second, Level::Scene, None)
                .map(|_| ())
        })
        .unwrap();
        p.set_active_page(second).unwrap();
        layer(&mut p, "Hero");
        // Locking a scene protects its panels; Undo restores the lock state.
        let scene = p.storyboard().unwrap().panels[&second].scene;
        p.edit_storyboard(|b| {
            b.scenes.get_mut(&scene).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        assert!(p.is_read_only());
        assert!(p.undo());
        assert!(!p.is_read_only());
        p.edit_storyboard(|b| {
            b.panels.get_mut(&first).unwrap().locked = false;
            Ok(())
        })
        .unwrap();
        p.remove_page(first).unwrap();
        valid(&p);
    }

    #[test]
    fn regrouping_never_drops_a_lock() {
        let mut p = board(4);
        let ids = layout(&p);
        p.edit_storyboard(|b| b.split(&ids, ids[2], Level::Scene, None).map(|_| ()))
            .unwrap();
        let locked = p.storyboard().unwrap().panels[&ids[2]].scene;
        p.edit_storyboard(|b| {
            b.scenes.get_mut(&locked).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        let stamp = p.stamp();
        // Joining it away, moving a panel out, or dragging a page out is refused.
        assert!(
            p.edit_storyboard(|b| b.join(&ids, locked).map(|_| ()))
                .is_err()
        );
        assert!(p.move_panels(&[ids[2]], 0, None).is_err());
        assert!(p.move_page(ids[3], 0).is_err());
        assert_eq!(p.stamp(), stamp);
        // Moving within the scene is fine, and so is unlocking the scene while
        // setting a panel's own lock in the same step.
        p.move_page(ids[3], 2).unwrap();
        p.edit_storyboard(|b| {
            b.scenes.get_mut(&locked).unwrap().locked = false;
            b.panels.get_mut(&ids[2]).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        assert!(p.storyboard().unwrap().is_locked(ids[2]));
        assert!(!p.storyboard().unwrap().is_locked(ids[3]));
        valid(&p);
    }

    #[test]
    fn several_panels_are_removed_in_one_step() {
        let mut p = board(4);
        let ids = layout(&p);
        p.set_active_page(ids[1]).unwrap();
        p.remove_pages(&[ids[1], ids[2]]).unwrap();
        assert_eq!(layout(&p), [ids[0], ids[3]]);
        assert_eq!(p.active_page(), ids[3]);
        assert!(p.remove_pages(&[ids[0], ids[3]]).is_err());
        assert!(p.undo());
        assert_eq!(layout(&p), ids);
        valid(&p);
    }

    #[test]
    fn locks_survive_saving_and_opening() {
        let mut p = board(1);
        p.edit_storyboard(|b| {
            b.panels.get_mut(&1).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        let reopened = ProjectEditor::open(p.snapshot().unwrap(), None).unwrap();
        assert!(reopened.is_read_only());
    }

    #[test]
    fn smart_add_carries_listed_layers_into_the_next_panel() {
        let mut p = board(2);
        let first = layout(&p)[0];
        p.set_active_page(first).unwrap();
        layer(&mut p, "Set");
        layer(&mut p, "Hero");
        p.edit_storyboard(|b| {
            b.smart_add_layers = vec!["set".into()];
            b.panels.get_mut(&first).unwrap().size = crate::storyboard::ShotSize::Wide;
            Ok(())
        })
        .unwrap();
        let added = p.smart_add_panel(first).unwrap();
        assert_eq!(layout(&p)[1], added);
        let names: Vec<_> = p.doc.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(
            names.contains(&"Set") && !names.contains(&"Hero"),
            "{names:?}"
        );
        let board = p.storyboard().unwrap();
        assert_eq!(board.panels[&added].scene, board.panels[&first].scene);
        assert_eq!(board.panels[&added].size, crate::storyboard::ShotSize::Wide);
        assert!(board.panels[&added].captions.is_empty());
        valid(&p);
        assert!(p.undo());
        assert_eq!(p.page_list().len(), 2);
    }

    #[test]
    fn panels_move_between_scenes_and_scenes_split_and_join() {
        let mut p = board(4);
        let ids = layout(&p);
        let second = p
            .storyboard()
            .unwrap()
            .clone()
            .split(&ids, ids[2], Level::Scene, None)
            .unwrap();
        p.edit_storyboard(|b| b.split(&ids, ids[2], Level::Scene, None).map(|_| ()))
            .unwrap();
        let first = p.storyboard().unwrap().panels[&ids[0]].scene;
        assert_eq!(p.storyboard().unwrap().panels[&ids[2]].scene, second);
        // The inserted scene is named after the one it came from.
        assert_eq!(p.storyboard().unwrap().scenes[&second].name, "1A");
        // Drag the second panel into the next scene, at its start.
        p.move_panels(&[ids[1]], 1, Some(second)).unwrap();
        let board = p.storyboard().unwrap();
        assert_eq!(board.panels[&ids[1]].scene, second);
        assert_eq!(board.outline(&layout(&p))[0].panels, [ids[0]]);
        // Landing away from the scene is refused.
        let stamp = p.stamp();
        assert!(p.move_panels(&[ids[3]], 0, Some(second)).is_err());
        assert_eq!(p.stamp(), stamp);
        // Joining merges the scene into the one before it.
        p.edit_storyboard(|b| b.join(&ids, second).map(|_| ()))
            .unwrap();
        let board = p.storyboard().unwrap();
        assert_eq!(board.scenes.len(), 1);
        assert!(board.panels.values().all(|p| p.scene == first));
        assert!(
            p.storyboard()
                .unwrap()
                .clone()
                .join(&layout(&p), first)
                .is_err()
        );
        valid(&p);
    }

    #[test]
    fn renumbering_follows_the_rules_for_all_or_part_of_the_board() {
        let mut p = board(3);
        let ids = layout(&p);
        p.edit_storyboard(|b| {
            b.naming = Naming {
                scene_prefix: "SC".into(),
                scene_start: 10,
                scene_step: 10,
                scene_digits: 3,
                ..Naming::default()
            };
            b.split(&ids, ids[1], Level::Scene, Some("Chase"))
                .map(|_| ())
        })
        .unwrap();
        let changed = p.renumber(&RenumberScope::All, true, true).unwrap();
        assert!(changed > 0);
        let board = p.storyboard().unwrap();
        let outline = board.outline(&ids);
        assert_eq!(board.scenes[&outline[0].scene].name, "SC010");
        assert_eq!(board.scenes[&outline[1].scene].name, "SC020");
        let names: Vec<_> = p.page_list().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Panel 1", "Panel 1", "Panel 2"]);
        assert_eq!(p.renumber(&RenumberScope::All, true, true).unwrap(), 0);
        // Part of the board keeps its place in the whole.
        let second = outline[1].scene;
        p.edit_storyboard(|b| b.rename(second, "X")).unwrap();
        p.renumber(&RenumberScope::Groups(vec![second]), true, false)
            .unwrap();
        assert_eq!(p.storyboard().unwrap().scenes[&second].name, "SC020");
        assert!(p.undo());
        assert_eq!(p.storyboard().unwrap().scenes[&second].name, "X");
    }

    #[test]
    fn thumbnail_sheets_become_one_panel_per_cell() {
        let mut p = board(2);
        let [sheet, last] = layout(&p)[..] else {
            unreachable!()
        };
        p.edit_storyboard(|b| {
            b.panels.get_mut(&sheet).unwrap().thumbnails = Some(ThumbnailGrid {
                columns: 2,
                rows: 1,
                gap: 4,
                margin: 4,
            });
            Ok(())
        })
        .unwrap();
        assert_eq!(p.storyboard().unwrap().total_frames(), 24);
        assert!(p.convert_thumbnails(last).is_err());
        let ids = p.convert_thumbnails(sheet).unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(layout(&p), [ids[0], ids[1], last]);
        for id in &ids {
            let doc = &p.page(*id).unwrap().doc;
            assert_eq!((doc.width, doc.height), (64, 36));
            assert!(p.storyboard().unwrap().panels[id].thumbnails.is_none());
        }
        valid(&p);
        assert!(p.undo());
        assert_eq!(layout(&p), [sheet, last]);
    }

    #[test]
    fn panels_and_whole_scenes_paste_into_another_storyboard() {
        let mut source = board(3);
        let ids = layout(&source);
        source
            .edit_storyboard(|b| {
                b.settings.frame_rate = FrameRate::whole(12);
                let dialogue = b.caption("Dialogue").unwrap();
                let note = b.add_caption_field("Sound", false, true)?;
                let panel = b.panels.get_mut(&ids[0]).unwrap();
                panel.frames = 12;
                panel.captions.insert(dialogue, "Hi".into());
                panel.captions.insert(note, "Rain".into());
                b.split(&ids, ids[1], Level::Scene, Some("Street"))
                    .map(|_| ())
            })
            .unwrap();
        // A whole scene, and part of one.
        let whole = source.copy_panels(&[ids[1], ids[2]]).unwrap();
        assert!(whole.whole_scenes && whole.scenes == ["Street"]);
        let part = source.copy_panels(&[ids[0]]).unwrap();
        assert!(part.whole_scenes);
        let part = PanelClip {
            whole_scenes: false,
            ..part
        };

        let mut target =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(128, 72)).unwrap();
        let pasted = target.paste_panels(Some(1), &whole).unwrap();
        let board = target.storyboard().unwrap();
        assert_eq!(board.outline(&layout(&target)).len(), 2);
        assert_eq!(board.scenes[&board.panels[&pasted[0]].scene].name, "Street");
        let doc = &target.page(pasted[0]).unwrap().doc;
        assert_eq!((doc.width, doc.height), (128, 72));

        let one = target.paste_panels(Some(1), &part).unwrap()[0];
        let board = target.storyboard().unwrap();
        // 12 frames at 12 fps is one second: 24 frames at 24 fps.
        assert_eq!(board.panels[&one].frames, 24);
        assert_eq!(board.panels[&one].scene, board.panels[&1].scene);
        let sound = board.caption("Sound").unwrap();
        assert_eq!(board.panels[&one].captions[&sound].text, "Rain");
        valid(&target);
        assert!(target.undo());
        assert!(target.storyboard().unwrap().caption("Sound").is_none());
        assert!(target.undo());
        assert_eq!(target.page_list().len(), 1);
    }

    #[test]
    fn find_and_replace_skips_locked_panels_and_undoes_in_one_step() {
        let mut p = board(2);
        let ids = layout(&p);
        p.edit_storyboard(|b| {
            let action = b.caption("Action").unwrap();
            for id in &ids {
                b.panels
                    .get_mut(id)
                    .unwrap()
                    .captions
                    .insert(action, "Mia runs. MIA stops.".into());
            }
            b.panels.get_mut(&ids[1]).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        let board = p.storyboard().unwrap();
        assert_eq!(
            board.find(&ids, "mia", None, FindOptions::default()).len(),
            4
        );
        let mut counts = (0, 0);
        p.edit_storyboard(|b| {
            counts = b.replace_all(&ids, "mia", "Tom", None, FindOptions::default());
            Ok(())
        })
        .unwrap();
        assert_eq!(counts, (2, 1));
        let board = p.storyboard().unwrap();
        let action = board.caption("Action").unwrap();
        assert_eq!(
            board.panels[&ids[0]].captions[&action].text,
            "Tom runs. Tom stops."
        );
        assert_eq!(
            board.panels[&ids[1]].captions[&action].text,
            "Mia runs. MIA stops."
        );
        assert!(p.undo());
        let action_text = &p.storyboard().unwrap().panels[&ids[0]].captions[&action].text;
        assert_eq!(action_text, "Mia runs. MIA stops.");
    }

    #[test]
    fn caption_fields_are_added_reordered_and_removed() {
        let mut p = board(1);
        p.edit_storyboard(|b| {
            let id = b.add_caption_field("Camera", false, true)?;
            assert!(b.add_caption_field("camera", false, true).is_err());
            b.panels
                .get_mut(&1)
                .unwrap()
                .captions
                .insert(id, "Pan".into());
            b.move_caption_field(id, 0)
        })
        .unwrap();
        let board = p.storyboard().unwrap();
        assert_eq!(board.captions[0].name, "Camera");
        let id = board.captions[0].id;
        p.edit_storyboard(|b| b.remove_caption_field(id)).unwrap();
        assert!(p.storyboard().unwrap().panels[&1].captions.is_empty());
        valid(&p);
    }

    #[test]
    fn new_storyboards_start_from_preferences() {
        use crate::creation::{CanvasKind, CanvasSpec};
        let preferences = crate::storyboard::Preferences {
            naming: Naming {
                panel_prefix: "P".into(),
                ..Naming::default()
            },
            panel_seconds: 1.5,
            captions: vec![crate::storyboard::CaptionPreset {
                name: "Beat".into(),
                multiline: true,
                print: true,
            }],
            ..Default::default()
        };
        let p = CanvasSpec {
            name: "Board".into(),
            kind: CanvasKind::Storyboard,
            width: 64.,
            height: 36.,
            pages: 2,
            ..Default::default()
        }
        .create_project_with(&preferences)
        .unwrap();
        let board = p.storyboard().unwrap();
        assert_eq!(board.settings.panel_frames, 36);
        assert!(board.panels.values().all(|p| p.frames == 36));
        assert_eq!(board.captions.len(), 1);
        assert_eq!(p.page_list()[1].name, "P2");
        valid(&p);
    }
}
