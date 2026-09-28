//! Reusable native object groups. Definitions live in hidden source groups;
//! instances remain ordinary editable layers and publish only on explicit update.
use crate::{Command, Document, Editor, NodeId, command::Slot, design_metadata::Design};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    pub variants: BTreeMap<String, NodeId>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub component: String,
    pub variant: String,
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 80 && !name.chars().any(char::is_control)
}
pub fn source_roots(design: &Design) -> HashSet<NodeId> {
    design
        .components
        .values()
        .flat_map(|d| d.variants.values().copied())
        .collect()
}
pub fn validate(design: &Design, doc: &Document) -> Result<(), String> {
    if design.components.len() > 128 {
        return Err("A page supports up to 128 components.".into());
    }
    let mut sources = HashSet::new();
    for (name, definition) in &design.components {
        if !valid_name(name) || definition.variants.is_empty() || definition.variants.len() > 32 {
            return Err("Components need a name and 1–32 variants.".into());
        }
        for (variant, root) in &definition.variants {
            if !valid_name(variant)
                || !sources.insert(*root)
                || !doc
                    .node(*root)
                    .is_some_and(|n| n.is_group() && !n.visible && n.parent.is_none())
            {
                return Err("Component sources must be separate hidden top-level groups.".into());
            }
            if doc
                .subtree(*root)
                .iter()
                .any(|id| design.component_links.contains_key(id))
            {
                return Err(
                    "Nested linked components are not supported. Detach nested instances first."
                        .into(),
                );
            }
        }
    }
    for (id, link) in &design.component_links {
        if !doc.node(*id).is_some_and(|n| n.is_group())
            || !design
                .components
                .get(&link.component)
                .is_some_and(|d| d.variants.contains_key(&link.variant))
        {
            return Err("Component instance refers to a missing source or variant.".into());
        }
        if doc
            .subtree(*id)
            .iter()
            .any(|child| child != id && design.component_links.contains_key(child))
        {
            return Err("Nested linked components are not supported.".into());
        }
    }
    Ok(())
}
/// Merge remapped clipboard libraries, preserving conflicting names independently.
pub fn merge_into(target: &mut Design, incoming: &Design) {
    let mut names = BTreeMap::new();
    for (name, definition) in &incoming.components {
        let mut chosen = name.clone();
        let stem: String = name.chars().take(64).collect();
        let mut i = 2;
        while target
            .components
            .get(&chosen)
            .is_some_and(|d| d != definition)
        {
            chosen = format!("{stem} ({i})");
            i += 1;
        }
        target.components.insert(chosen.clone(), definition.clone());
        names.insert(name.clone(), chosen);
    }
    for (id, link) in &incoming.component_links {
        let mut link = link.clone();
        if let Some(name) = names.get(&link.component) {
            link.component = name.clone();
        }
        if target.components.contains_key(&link.component) {
            target.component_links.insert(*id, link);
        }
    }
}
struct Plan {
    doc: Document,
    commands: Vec<Command>,
}
impl Plan {
    fn new(editor: &Editor) -> Result<Self, String> {
        if editor.in_transaction() {
            return Err("Finish the current edit before changing components.".into());
        }
        Ok(Self {
            doc: editor.doc.clone(),
            commands: Vec::new(),
        })
    }
    fn run(&mut self, command: Command) -> Result<Option<NodeId>, String> {
        let id = command.apply(&mut self.doc).map_err(|e| e.to_string())?;
        self.commands.push(command);
        Ok(id)
    }
    fn design(&mut self, design: Design) -> Result<(), String> {
        self.run(Command::SetDesign {
            design: Box::new(design),
        })?;
        Ok(())
    }
    fn commit(self, editor: &mut Editor, label: &str) -> Result<(), String> {
        editor.begin(label);
        for command in self.commands {
            if let Err(e) = editor.execute(command) {
                editor.cancel();
                return Err(e.to_string());
            }
        }
        editor.doc.retain_raw_originals(&self.doc);
        editor.end();
        Ok(())
    }
}
fn editable(doc: &Document, root: NodeId) -> Result<(), String> {
    if doc.node(root).is_none() {
        return Err("Select an existing object.".into());
    }
    for id in doc.subtree(root) {
        let locks = doc.layer_locks(id);
        if doc.locked_ancestor(id).is_some() || locks.pixels || locks.position || locks.transparency
        {
            return Err("Unlock every affected component object before updating it.".into());
        }
    }
    Ok(())
}
fn standalone(doc: &Document, ids: &[NodeId]) -> Result<(), String> {
    if ids.is_empty() {
        return Err("Select objects to create a component.".into());
    }
    let sources = source_roots(&doc.design);
    for id in ids {
        editable(doc, *id)?;
        if doc
            .subtree(*id)
            .iter()
            .any(|n| doc.design.component_links.contains_key(n) || sources.contains(n))
            || doc
                .design
                .component_links
                .keys()
                .any(|n| doc.is_ancestor(*n, *id))
        {
            return Err("Detach existing instances before creating a component from them.".into());
        }
    }
    Ok(())
}
fn copy_group(
    plan: &mut Plan,
    source: &Document,
    root: NodeId,
    target: Option<NodeId>,
    hidden: bool,
) -> Result<NodeId, String> {
    if !source.node(root).is_some_and(|node| node.is_group()) {
        return Err("Component source group is missing.".into());
    }
    plan.doc.retain_raw_originals(source);
    let ids: HashSet<_> = source.subtree(root).into_iter().collect();
    let mut additions = source.design.fragment(&ids);
    additions.components.clear();
    additions.component_links.clear();
    let mut map = HashMap::new();
    if let Some(target) = target {
        editable(&plan.doc, target)?;
        let children = plan.doc.children(Some(target));
        for child in children {
            plan.run(Command::RemoveNode { id: child })?;
        }
        map.insert(root, target);
        for command in crate::design_appearance::Appearance::capture(
            source.node(root).ok_or("Missing source")?,
        )
        .commands(plan.doc.node(target).unwrap())
        {
            plan.run(command)?;
        }
        let node = source.node(root).unwrap();
        plan.run(Command::SetMask {
            id: target,
            mask: node.mask.clone(),
        })?;
        if node.mask.is_some() {
            plan.run(Command::SetMaskEnabled {
                id: target,
                enabled: node.mask_enabled,
            })?;
            plan.run(Command::SetMaskLinked {
                id: target,
                linked: node.mask_linked,
            })?;
            plan.run(Command::SetMaskTransform {
                id: target,
                transform: node.mask_transform,
            })?;
        }
    }
    let mut waiting: Vec<_> = source
        .nodes
        .iter()
        .filter(|n| ids.contains(&n.id) && !map.contains_key(&n.id))
        .cloned()
        .collect();
    while !waiting.is_empty() {
        let before = waiting.len();
        let mut next = Vec::new();
        for original in waiting {
            if original.id != root && original.parent.is_some_and(|id| !map.contains_key(&id)) {
                next.push(original);
                continue;
            }
            let mut node = original.clone();
            node.link_group = None;
            node.locked = false;
            node.locks = Default::default();
            let slot = if original.id == root {
                node.visible = !hidden;
                node.name = if hidden {
                    format!("Component source · {}", node.name)
                } else {
                    node.name
                };
                Slot::TOP
            } else {
                Slot::top_of(original.parent.map(|id| map[&id]))
            };
            // Native vector caches follow the destination canvas size.
            match &mut node.kind {
                crate::NodeKind::Text { spec, cache } => {
                    *cache = crate::vector_cache::VectorRaster::text(
                        spec.clone(),
                        plan.doc.width,
                        plan.doc.height,
                    )
                }
                crate::NodeKind::Path { path, style, cache } => {
                    *cache = crate::vector_cache::VectorRaster::path(
                        path.clone(),
                        *style,
                        plan.doc.width,
                        plan.doc.height,
                    )
                }
                _ => {}
            }
            let id = plan
                .run(Command::AddNode {
                    node: Box::new(node),
                    slot,
                })?
                .ok_or("Could not clone component")?;
            map.insert(original.id, id);
        }
        if next.len() == before {
            return Err("Invalid component hierarchy.".into());
        }
        waiting = next;
    }
    for node in source
        .nodes
        .iter()
        .filter(|n| ids.contains(&n.id) && n.id != root)
    {
        if let Some(base) = node.clip_to.and_then(|id| map.get(&id)) {
            plan.run(Command::SetClip {
                id: map[&node.id],
                clip_to: Some(*base),
            })?;
        }
    }
    let additions = additions.remap(&map);
    let mut design = plan.doc.design.clone();
    // A reset must remove root semantics that no longer exist in the variant.
    let dest = map[&root];
    design.charts.remove(&dest);
    design.frames.remove(&dest);
    design.media.remove(&dest);
    design.constraints.remove(&dest);
    design.motion.remove(&dest);
    design.style_links.remove(&dest);
    crate::design_styles::merge_into(&mut design, &additions);
    design.charts.extend(additions.charts);
    design.frames.extend(additions.frames);
    design.media.extend(additions.media);
    design.constraints.extend(additions.constraints);
    for (id, motion) in additions.motion {
        design.duration_ms = design.duration_ms.max(motion.end_ms);
        design.motion.insert(id, motion);
    }
    plan.design(design)?;
    if let Some(diagram) = &source.diagram {
        let additions = diagram.fragment(&ids).remap(&map);
        let mut model = plan.doc.diagram.as_deref().cloned().unwrap_or_default();
        model.shapes.extend(additions.shapes);
        model.edges.extend(additions.edges);
        plan.run(Command::SetDiagram {
            diagram: Some(std::sync::Arc::new(model)),
        })?;
    }
    Ok(dest)
}
pub fn create(editor: &mut Editor, ids: &[NodeId], name: &str) -> Result<NodeId, String> {
    let name = name.trim();
    if !valid_name(name) || editor.doc.design.components.contains_key(name) {
        return Err("Choose a unique component name of 1–80 characters.".into());
    }
    standalone(&editor.doc, ids)?;
    let mut plan = Plan::new(editor)?;
    let root = if ids.len() == 1 && plan.doc.node(ids[0]).is_some_and(|n| n.is_group()) {
        ids[0]
    } else {
        plan.run(Command::Group {
            ids: ids.to_vec(),
            name: name.into(),
        })?
        .ok_or("Could not group selection")?
    };
    let snapshot = plan.doc.clone();
    let source = copy_group(&mut plan, &snapshot, root, None, true)?;
    let mut design = plan.doc.design.clone();
    design.components.insert(
        name.into(),
        Definition {
            variants: BTreeMap::from([("Default".into(), source)]),
        },
    );
    design.component_links.insert(
        root,
        Instance {
            component: name.into(),
            variant: "Default".into(),
        },
    );
    plan.design(design)?;
    plan.commit(editor, "Create reusable component")?;
    Ok(root)
}
fn insert_into(
    plan: &mut Plan,
    name: &str,
    variant: &str,
    offset: (f64, f64),
) -> Result<NodeId, String> {
    let source = *plan
        .doc
        .design
        .components
        .get(name)
        .and_then(|d| d.variants.get(variant))
        .ok_or("Component variant is missing")?;
    let snapshot = plan.doc.clone();
    let id = copy_group(plan, &snapshot, source, None, false)?;
    plan.run(Command::Rename {
        id,
        name: name.into(),
    })?;
    plan.run(Command::TranslateNodes {
        ids: vec![id],
        dx: offset.0,
        dy: offset.1,
    })?;
    let mut design = plan.doc.design.clone();
    design.component_links.insert(
        id,
        Instance {
            component: name.into(),
            variant: variant.into(),
        },
    );
    plan.design(design)?;
    Ok(id)
}
pub fn insert(
    editor: &mut Editor,
    name: &str,
    variant: &str,
    offset: (f64, f64),
) -> Result<NodeId, String> {
    let mut plan = Plan::new(editor)?;
    let id = insert_into(&mut plan, name, variant, offset)?;
    plan.commit(editor, "Insert component")?;
    Ok(id)
}
fn replace_instance(
    plan: &mut Plan,
    snapshot: &Document,
    source: NodeId,
    target: NodeId,
) -> Result<(), String> {
    if plan
        .doc
        .node(target)
        .is_some_and(|node| node.link_group.is_some())
    {
        return Err("Unlink the instance's movement links before resetting or updating it.".into());
    }
    let old = crate::geometry::node_bounds(&plan.doc, target)
        .ok_or("Component has no visible geometry")?;
    copy_group(plan, snapshot, source, Some(target), false)?;
    let new = crate::geometry::node_bounds(&plan.doc, target)
        .ok_or("Component variant has no geometry")?;
    plan.run(Command::TranslateNodes {
        ids: vec![target],
        dx: (old.x - new.x) as f64,
        dy: (old.y - new.y) as f64,
    })?;
    Ok(())
}
pub fn reset(editor: &mut Editor, id: NodeId, variant: Option<&str>) -> Result<(), String> {
    let mut plan = Plan::new(editor)?;
    let mut link = plan
        .doc
        .design
        .component_links
        .get(&id)
        .cloned()
        .ok_or("Select a linked component group")?;
    if let Some(v) = variant {
        link.variant = v.into();
    }
    let source = *plan.doc.design.components[&link.component]
        .variants
        .get(&link.variant)
        .ok_or("Unknown variant")?;
    let snapshot = plan.doc.clone();
    replace_instance(&mut plan, &snapshot, source, id)?;
    let mut design = plan.doc.design.clone();
    design.component_links.insert(id, link);
    plan.design(design)?;
    plan.commit(editor, "Reset component variant")
}
pub fn detach(editor: &mut Editor, id: NodeId) -> Result<(), String> {
    let mut plan = Plan::new(editor)?;
    editable(&plan.doc, id)?;
    let mut design = plan.doc.design.clone();
    if design.component_links.remove(&id).is_none() {
        return Err("Select a linked component group".into());
    }
    plan.design(design)?;
    plan.commit(editor, "Detach component")
}
/// Publish edited artwork to its variant, or save a new named variant. Linked
/// instances on this page update together; other variants remain untouched.
pub fn update(editor: &mut Editor, id: NodeId, new_variant: Option<&str>) -> Result<(), String> {
    let mut plan = Plan::new(editor)?;
    editable(&plan.doc, id)?;
    let mut link = plan
        .doc
        .design
        .component_links
        .get(&id)
        .cloned()
        .ok_or("Select a linked component group")?;
    if let Some(name) = new_variant {
        let name = name.trim();
        if !valid_name(name)
            || plan.doc.design.components[&link.component]
                .variants
                .contains_key(name)
        {
            return Err("Choose a unique variant name of 1–80 characters.".into());
        }
        link.variant = name.into();
    }
    let targets: Vec<_> = plan
        .doc
        .design
        .component_links
        .iter()
        .filter(|(_, l)| **l == link)
        .map(|(id, _)| *id)
        .collect();
    for target in &targets {
        editable(&plan.doc, *target)?;
    }
    let snapshot = plan.doc.clone();
    let previous = plan.doc.design.components[&link.component]
        .variants
        .get(&link.variant)
        .copied();
    let source = copy_group(&mut plan, &snapshot, id, previous, true)?;
    let mut design = plan.doc.design.clone();
    design
        .components
        .get_mut(&link.component)
        .unwrap()
        .variants
        .insert(link.variant.clone(), source);
    design.component_links.insert(id, link.clone());
    plan.design(design)?;
    for target in targets {
        if target != id {
            replace_instance(&mut plan, &snapshot, id, target)?;
        }
    }
    plan.commit(
        editor,
        if new_variant.is_some() {
            "Save component variant"
        } else {
            "Update linked component"
        },
    )
}
/// Import a page's complete variant library as an independent local definition.
fn import_into(plan: &mut Plan, source: &Document, name: &str) -> Result<String, String> {
    let definition = source
        .design
        .components
        .get(name)
        .ok_or("Component no longer exists on that page")?;
    let stem: String = name.chars().take(64).collect();
    let mut chosen = name.to_string();
    let mut i = 2;
    while plan.doc.design.components.contains_key(&chosen) {
        chosen = format!("{stem} ({i})");
        i += 1;
    }
    let mut variants = BTreeMap::new();
    for (variant, root) in &definition.variants {
        variants.insert(
            variant.clone(),
            copy_group(plan, source, *root, None, true)?,
        );
    }
    let mut design = plan.doc.design.clone();
    design
        .components
        .insert(chosen.clone(), Definition { variants });
    plan.design(design)?;
    Ok(chosen)
}
pub fn import(editor: &mut Editor, source: &Document, name: &str) -> Result<String, String> {
    let mut plan = Plan::new(editor)?;
    let name = import_into(&mut plan, source, name)?;
    plan.commit(editor, "Import component library")?;
    Ok(name)
}
/// Import and place across pages as one atomic Undo step.
pub fn import_and_insert(
    editor: &mut Editor,
    source: &Document,
    name: &str,
    variant: &str,
    offset: (f64, f64),
) -> Result<NodeId, String> {
    let mut plan = Plan::new(editor)?;
    let name = import_into(&mut plan, source, name)?;
    let id = insert_into(&mut plan, &name, variant, offset)?;
    plan.commit(editor, "Insert component from another page")?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, NodeKind, fragment::Fragment, text::TextSpec};
    fn setup() -> (Editor, NodeId) {
        let mut editor = Editor::new(Document::new(800, 600), None);
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Title",
                    TextSpec {
                        text: "Original".into(),
                        x: 40.,
                        y: 60.,
                        ..Default::default()
                    },
                    800,
                    600,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let root = create(&mut editor, &[id], "Card").unwrap();
        (editor, root)
    }
    fn text(doc: &Document, root: NodeId) -> String {
        doc.subtree(root)
            .iter()
            .find_map(|id| match &doc.node(*id)?.kind {
                NodeKind::Text { spec, .. } => Some(spec.text.clone()),
                _ => None,
            })
            .unwrap()
    }
    fn edit(editor: &mut Editor, root: NodeId, value: &str) {
        let id = editor.doc.children(Some(root))[0];
        let NodeKind::Text { spec, .. } = &editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        let mut spec = (**spec).clone();
        spec.text = value.into();
        editor
            .execute(Command::SetText {
                id,
                spec: Box::new(spec),
            })
            .unwrap();
    }
    #[test]
    fn component_publish_reset_variants_identity_and_undo() {
        let (mut e, a) = setup();
        let b = insert(&mut e, "Card", "Default", (200., 0.)).unwrap();
        let bounds = crate::geometry::node_bounds(&e.doc, b).unwrap();
        edit(&mut e, a, "Published");
        let before = e.doc.clone();
        update(&mut e, a, None).unwrap();
        assert_eq!(text(&e.doc, b), "Published");
        assert_eq!(crate::geometry::node_bounds(&e.doc, b).unwrap().x, bounds.x);
        assert!(e.doc.node(b).is_some());
        e.undo();
        assert_eq!(e.doc, before);
        e.redo();
        edit(&mut e, a, "Alternate");
        update(&mut e, a, Some("Dark")).unwrap();
        assert_eq!(text(&e.doc, b), "Published");
        reset(&mut e, b, Some("Dark")).unwrap();
        assert_eq!(text(&e.doc, b), "Alternate");
        detach(&mut e, b).unwrap();
        assert_eq!(text(&e.doc, b), "Alternate");
        assert!(!e.doc.design.component_links.contains_key(&b));
        e.doc.validate().unwrap();
    }
    #[test]
    fn protected_publish_is_atomic_and_nesting_is_rejected() {
        let (mut e, a) = setup();
        let b = insert(&mut e, "Card", "Default", (200., 0.)).unwrap();
        let child = e.doc.children(Some(b))[0];
        e.execute(Command::SetLocked {
            id: child,
            locked: true,
        })
        .unwrap();
        edit(&mut e, a, "Changed");
        let before = e.doc.clone();
        let history = e.history.len();
        assert!(update(&mut e, a, None).is_err());
        assert_eq!(e.doc, before);
        assert_eq!(e.history.len(), history);
        assert!(create(&mut e, &[a], "Cycle").is_err());
    }
    #[test]
    fn clipboard_keeps_hidden_library_editability_and_cross_page_import() {
        let (mut e, a) = setup();
        update(&mut e, a, Some("Second")).unwrap();
        let fragment = Fragment::capture(&e.doc, &[a]).unwrap();
        let mut target = Editor::new(Document::new(400, 300), None);
        let ids = fragment.paste(&mut target, Slot::TOP, (0., 0.)).unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(text(&target.doc, ids[0]), "Original");
        assert_eq!(target.doc.design.components["Card"].variants.len(), 2);
        for root in source_roots(&target.doc.design) {
            assert!(!target.doc.node(root).unwrap().visible);
            assert!(target.doc.node(root).unwrap().parent.is_none());
        }
        target.doc.validate().unwrap();
        let mut page = Editor::new(Document::new(1000, 600), None);
        let name = import(&mut page, &e.doc, "Card").unwrap();
        let id = insert(&mut page, &name, "Second", (0., 0.)).unwrap();
        assert_eq!(text(&page.doc, id), "Original");
        page.doc.validate().unwrap();
    }
    #[test]
    fn cross_page_insert_is_one_undo_and_invalid_variant_is_atomic() {
        let (source, _) = setup();
        let mut target = Editor::new(Document::new(500, 300), None);
        let before = target.doc.clone();
        assert!(import_and_insert(&mut target, &source.doc, "Card", "Missing", (0., 0.)).is_err());
        assert_eq!(target.doc, before);
        assert!(target.history.is_empty());
        let root =
            import_and_insert(&mut target, &source.doc, "Card", "Default", (10., 20.)).unwrap();
        assert_eq!(target.history.len(), 1);
        assert_eq!(text(&target.doc, root), "Original");
        let source_root = *target.doc.design.components["Card"]
            .variants
            .values()
            .next()
            .unwrap();
        assert!(
            target
                .execute(Command::SetVisible {
                    id: source_root,
                    visible: true
                })
                .is_err()
        );
        target.undo();
        assert_eq!(target.doc, before);
        target.redo();
        target.doc.validate().unwrap();
    }

    #[test]
    fn library_survives_last_instance_deletion_and_duplicate_links() {
        let (mut e, a) = setup();
        let b = e
            .execute(Command::DuplicateNode { id: a })
            .unwrap()
            .unwrap();
        assert_eq!(
            e.doc.design.component_links.get(&a),
            e.doc.design.component_links.get(&b)
        );
        e.execute(Command::RemoveNode { id: a }).unwrap();
        e.execute(Command::RemoveNode { id: b }).unwrap();
        assert_eq!(e.doc.design.components.len(), 1);
        assert!(e.doc.design.component_links.is_empty());
        insert(&mut e, "Card", "Default", (0., 0.)).unwrap();
    }
}
