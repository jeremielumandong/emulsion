//! Reusable native object groups. Definitions live in hidden source groups;
//! instances remain ordinary editable layers and publish only on explicit update.
#[cfg(test)]
use crate::command::Slot;
use crate::{Command, Document, Editor, NodeId, design_metadata::Design};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    #[serde(default)]
    pub library_id: String,
    #[serde(default)]
    pub member_keys: BTreeMap<NodeId, String>,
    pub variants: BTreeMap<String, NodeId>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    /// New instances track native edits; absent in legacy files means manual.
    #[serde(default)]
    pub auto_overrides: bool,
    pub component: String,
    pub variant: String,
    /// Source member IDs map to stable native IDs in this instance.
    #[serde(default)]
    pub members: BTreeMap<NodeId, NodeId>,
    #[serde(default)]
    pub overrides: BTreeMap<NodeId, Overrides>,
}
#[path = "design_component_overrides.rs"]
mod overrides;
pub use overrides::Overrides;
#[path = "design_component_nested.rs"]
mod nested;
impl Instance {
    pub fn remap(&self, map: &HashMap<NodeId, NodeId>) -> Self {
        let id = |id| map.get(&id).copied().unwrap_or(id);
        Self {
            auto_overrides: self.auto_overrides,
            component: self.component.clone(),
            variant: self.variant.clone(),
            members: self.members.iter().map(|(a, b)| (id(*a), id(*b))).collect(),
            overrides: self.overrides.iter().map(|(a, b)| (id(*a), *b)).collect(),
        }
    }
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
        nested::validate_members(design, doc, *id, link)?;
    }
    nested::dependency_order(design, doc)?;
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
}
impl Plan {
    fn new(editor: &Editor) -> Result<Self, String> {
        if editor.in_transaction() {
            return Err("Finish the current edit before changing components.".into());
        }
        Ok(Self {
            doc: editor.doc.clone(),
        })
    }
    fn run(&mut self, command: Command) -> Result<Option<NodeId>, String> {
        let id = command.apply(&mut self.doc).map_err(|e| e.to_string())?;
        Ok(id)
    }
    fn design(&mut self, design: Design) -> Result<(), String> {
        self.run(Command::SetDesign {
            design: Box::new(design),
        })?;
        Ok(())
    }
    fn commit(mut self, editor: &mut Editor, label: &str) -> Result<(), String> {
        project::refresh_keys(&mut self.doc);
        editor.commit_design_document(self.doc, label)
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
        if doc.subtree(*id).iter().any(|n| sources.contains(n))
            || sources.iter().any(|n| doc.is_ancestor(*n, *id))
        {
            return Err("Create components from visible artwork, not hidden source groups.".into());
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
    matches: &BTreeMap<NodeId, NodeId>,
    preserve: bool,
) -> Result<(NodeId, BTreeMap<NodeId, NodeId>), String> {
    nested::copy_group(plan, source, root, target, hidden, matches, preserve)
}

pub fn create(editor: &mut Editor, ids: &[NodeId], name: &str) -> Result<NodeId, String> {
    let name = name.trim();
    if !valid_name(name) || editor.doc.design.components.contains_key(name) {
        return Err("Choose a unique component name of 1–80 characters.".into());
    }
    standalone(&editor.doc, ids)?;
    let mut plan = Plan::new(editor)?;
    let root = if ids.len() == 1
        && plan.doc.node(ids[0]).is_some_and(|n| n.is_group())
        && !plan.doc.design.component_links.contains_key(&ids[0])
    {
        ids[0]
    } else {
        plan.run(Command::Group {
            ids: ids.to_vec(),
            name: name.into(),
        })?
        .ok_or("Could not group selection")?
    };
    let snapshot = plan.doc.clone();
    let (source, members) = copy_group(
        &mut plan,
        &snapshot,
        root,
        None,
        true,
        &BTreeMap::new(),
        false,
    )?;
    let mut design = plan.doc.design.clone();
    design.components.insert(
        name.into(),
        Definition {
            variants: BTreeMap::from([("Default".into(), source)]),
            ..Default::default()
        },
    );
    design.component_links.insert(
        root,
        Instance {
            auto_overrides: true,
            component: name.into(),
            variant: "Default".into(),
            members: members
                .into_iter()
                .map(|(instance, source)| (source, instance))
                .collect(),
            overrides: BTreeMap::new(),
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
    let (id, members) = copy_group(
        plan,
        &snapshot,
        source,
        None,
        false,
        &BTreeMap::new(),
        false,
    )?;
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
            auto_overrides: true,
            component: name.into(),
            variant: variant.into(),
            members,
            overrides: BTreeMap::new(),
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
    source: NodeId,
    target: NodeId,
    preserve: bool,
) -> Result<(), String> {
    let snapshot = plan.doc.clone();
    let old = crate::geometry::node_bounds(&snapshot, target).ok_or("Component has no geometry")?;
    let mut link = snapshot
        .design
        .component_links
        .get(&target)
        .cloned()
        .ok_or("Missing component link")?;
    let mapping = nested::members(&snapshot, target, &link)?;
    let (_, map) = copy_group(
        plan,
        &snapshot,
        source,
        Some(target),
        false,
        &mapping,
        preserve,
    )?;
    nested::prune_members(&mut plan.doc);
    let new = crate::geometry::node_bounds(&plan.doc, target)
        .ok_or("Component variant has no geometry")?;
    plan.run(Command::TranslateNodes {
        ids: vec![target],
        dx: f64::from(old.x - new.x),
        dy: f64::from(old.y - new.y),
    })?;
    if preserve {
        nested::restore_geometry(plan, &snapshot, target, &map, &link)?;
    } else {
        link.overrides.clear();
    }
    link.members = map;
    link.overrides.retain(|id, _| link.members.contains_key(id));
    plan.doc.design.component_links.insert(target, link);
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
    if let Some(variant) = variant {
        link.variant = variant.into();
    }
    let source = *plan.doc.design.components[&link.component]
        .variants
        .get(&link.variant)
        .ok_or("Unknown variant")?;
    // Variants may have distinct source IDs; switch uses the selected variant's
    // member identities, while explicit Reset clears this instance's overrides.
    let changing = plan.doc.design.component_links[&id].variant != link.variant;
    if changing {
        link.members.clear();
        link.overrides.clear();
    }
    plan.doc.design.component_links.insert(id, link);
    replace_instance(&mut plan, source, id, false)?;
    plan.commit(editor, "Reset component variant")
}

/// Select the innermost owning linked group for an editable member.
pub fn owner(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut current = Some(node);
    for _ in 0..=doc.nodes.len() {
        let id = current?;
        if doc.design.component_links.contains_key(&id) {
            return Some(id);
        }
        current = doc.node(id)?.parent;
    }
    None
}
/// Replace the explicit retained-property set for one native member. Turning
/// flags off permits future updates; Reset immediately restores source values.
pub fn set_overrides(
    editor: &mut Editor,
    instance: NodeId,
    node: NodeId,
    flags: Overrides,
) -> Result<(), String> {
    configure_overrides(editor, instance, node, flags, None)
}
/// Atomically configure member properties and optional instance tracking policy.
pub fn configure_overrides(
    editor: &mut Editor,
    instance: NodeId,
    node: NodeId,
    flags: Overrides,
    auto: Option<bool>,
) -> Result<(), String> {
    let mut plan = Plan::new(editor)?;
    editable(&plan.doc, instance)?;
    let mut link = plan
        .doc
        .design
        .component_links
        .get(&instance)
        .cloned()
        .ok_or("Select a linked component")?;
    if let Some(enabled) = auto {
        link.auto_overrides = enabled;
    }
    link.members = nested::members(&plan.doc, instance, &link)?;
    let source = *link
        .members
        .iter()
        .find(|(_, id)| **id == node)
        .map(|(source, _)| source)
        .ok_or(
            "The selected object is not a mapped component member. Publish new objects first.",
        )?;
    flags.validate(plan.doc.node(node).ok_or("Missing component member")?)?;
    if flags.is_empty() {
        link.overrides.remove(&source);
    } else {
        link.overrides.insert(source, flags);
    }
    plan.doc.design.component_links.insert(instance, link);
    plan.commit(editor, "Set component property overrides")
}
pub fn overrides_for(doc: &Document, instance: NodeId, node: NodeId) -> Overrides {
    doc.design
        .component_links
        .get(&instance)
        .and_then(|link| {
            link.members
                .iter()
                .find(|(_, id)| **id == node)
                .and_then(|(source, _)| link.overrides.get(source))
        })
        .copied()
        .unwrap_or_default()
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
    let old_members = nested::members(&plan.doc, id, &link)?;
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
    let previous = plan.doc.design.components[&link.component]
        .variants
        .get(&link.variant)
        .copied();
    let matches = if previous.is_some() {
        old_members
            .iter()
            .map(|(source, instance)| (*instance, *source))
            .collect()
    } else {
        BTreeMap::new()
    };
    let snapshot = plan.doc.clone();
    let (source, mapping) = copy_group(&mut plan, &snapshot, id, previous, true, &matches, false)?;
    plan.doc
        .design
        .components
        .get_mut(&link.component)
        .unwrap()
        .variants
        .insert(link.variant.clone(), source);
    let rekey: BTreeMap<_, _> = old_members
        .iter()
        .filter_map(|(old, instance)| mapping.get(instance).map(|new| (*old, *new)))
        .collect();
    link.overrides = link
        .overrides
        .into_iter()
        .filter_map(|(key, value)| rekey.get(&key).map(|key| (*key, value)))
        .collect();
    link.members = mapping
        .into_iter()
        .map(|(instance, source)| (source, instance))
        .collect();
    plan.doc.design.component_links.insert(id, link.clone());
    nested::prune_members(&mut plan.doc);
    let order = nested::dependency_order(&plan.doc.design, &plan.doc)?;
    let mut affected = HashSet::from([(link.component.clone(), link.variant.clone())]);
    for component in order {
        let variants = plan.doc.design.components[&component].variants.clone();
        for (variant, source) in variants {
            let key = (component.clone(), variant.clone());
            let depends = plan
                .doc
                .subtree(source)
                .iter()
                .filter_map(|id| plan.doc.design.component_links.get(id))
                .any(|link| affected.contains(&(link.component.clone(), link.variant.clone())));
            if !affected.contains(&key) && !depends {
                continue;
            }
            affected.insert(key);
            let targets: Vec<_> = plan
                .doc
                .design
                .component_links
                .iter()
                .filter(|(_, l)| l.component == component && l.variant == variant)
                .map(|(id, _)| *id)
                .collect();
            for target in targets {
                if target != id {
                    replace_instance(&mut plan, source, target, true)?;
                }
            }
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
    nested::import_into(plan, source, name)
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
    fn protected_publish_is_atomic_and_nesting_is_allowed() {
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
        assert!(create(&mut e, &[a], "Wrapper").is_ok());
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
            e.doc.design.component_links[&a].component,
            e.doc.design.component_links[&b].component
        );
        e.execute(Command::RemoveNode { id: a }).unwrap();
        e.execute(Command::RemoveNode { id: b }).unwrap();
        assert_eq!(e.doc.design.components.len(), 1);
        assert!(e.doc.design.component_links.is_empty());
        insert(&mut e, "Card", "Default", (0., 0.)).unwrap();
    }
}

#[cfg(test)]
#[path = "design_component_nested_tests.rs"]
mod nested_tests;

#[path = "design_component_project.rs"]
mod project;
pub use project::{insert_project, publish_project};

/// Change tracking policy without changing existing appearance overrides.
pub fn set_auto_overrides(
    editor: &mut Editor,
    instance: NodeId,
    enabled: bool,
) -> Result<(), String> {
    let mut plan = Plan::new(editor)?;
    editable(&plan.doc, instance)?;
    let link = plan
        .doc
        .design
        .component_links
        .get_mut(&instance)
        .ok_or("Select a linked component instance.")?;
    link.auto_overrides = enabled;
    plan.commit(editor, "Set automatic component overrides")
}
