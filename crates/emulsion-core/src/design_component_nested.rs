//! Stable member cloning and bounded dependency propagation.
use super::*;

pub(super) fn dependencies(design: &Design, doc: &Document, name: &str) -> HashSet<String> {
    design
        .components
        .get(name)
        .into_iter()
        .flat_map(|d| d.variants.values())
        .flat_map(|root| doc.subtree(*root))
        .filter_map(|id| design.component_links.get(&id).map(|l| l.component.clone()))
        .collect()
}
pub(super) fn dependency_order(design: &Design, doc: &Document) -> Result<Vec<String>, String> {
    fn visit(
        name: &str,
        design: &Design,
        doc: &Document,
        active: &mut HashSet<String>,
        done: &mut HashSet<String>,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        if done.contains(name) {
            return Ok(());
        }
        if active.len() >= 32 || !active.insert(name.into()) {
            return Err(
                "Component dependencies must be acyclic and at most 32 levels deep.".into(),
            );
        }
        for child in dependencies(design, doc, name) {
            visit(&child, design, doc, active, done, out)?;
        }
        active.remove(name);
        done.insert(name.into());
        out.push(name.into());
        Ok(())
    }
    let mut out = Vec::new();
    let mut done = HashSet::new();
    let mut active = HashSet::new();
    for name in design.components.keys() {
        visit(name, design, doc, &mut active, &mut done, &mut out)?;
    }
    Ok(out)
}
pub(super) fn validate_members(
    design: &Design,
    doc: &Document,
    root: NodeId,
    link: &Instance,
) -> Result<(), String> {
    let source = design
        .components
        .get(&link.component)
        .and_then(|d| d.variants.get(&link.variant))
        .ok_or("Missing component source")?;
    if !link.members.is_empty() && link.members.get(source) != Some(&root) {
        return Err("Component root member mapping must retain instance identity.".into());
    }
    let source_ids: HashSet<_> = doc.subtree(*source).into_iter().collect();
    let target_ids: HashSet<_> = doc.subtree(root).into_iter().collect();
    let mut seen = HashSet::new();
    for (from, to) in &link.members {
        if !source_ids.contains(from) || !target_ids.contains(to) || !seen.insert(*to) {
            return Err("Component member mapping contains a missing or duplicate member.".into());
        }
    }
    for (key, flags) in &link.overrides {
        let id = link
            .members
            .get(key)
            .ok_or("Override refers to a missing component member")?;
        flags.validate(doc.node(*id).ok_or("Missing overridden object")?)?;
    }
    Ok(())
}
/// Legacy links lacked member identities. Bootstrap by hierarchy position once;
/// newly authored and subsequently saved links always retain explicit identities.
pub(super) fn members(
    doc: &Document,
    root: NodeId,
    link: &Instance,
) -> Result<BTreeMap<NodeId, NodeId>, String> {
    if !link.members.is_empty() {
        return Ok(link.members.clone());
    }
    let source = *doc
        .design
        .components
        .get(&link.component)
        .and_then(|d| d.variants.get(&link.variant))
        .ok_or("Missing component source")?;
    fn pair(doc: &Document, a: NodeId, b: NodeId, out: &mut BTreeMap<NodeId, NodeId>) {
        if doc.node(a).map(|n| std::mem::discriminant(&n.kind))
            != doc.node(b).map(|n| std::mem::discriminant(&n.kind))
        {
            return;
        }
        out.insert(a, b);
        for (a, b) in doc.children(Some(a)).into_iter().zip(doc.children(Some(b))) {
            pair(doc, a, b, out);
        }
    }
    let mut out = BTreeMap::new();
    pair(doc, source, root, &mut out);
    Ok(out)
}
pub(super) fn prune_members(doc: &mut Document) {
    let existing: HashSet<_> = doc.nodes.iter().map(|n| n.id).collect();
    for link in doc.design.component_links.values_mut() {
        link.members
            .retain(|a, b| existing.contains(a) && existing.contains(b));
        link.overrides.retain(|a, _| link.members.contains_key(a));
    }
}
fn preserve_flags(
    old: &Document,
    new: &Document,
    root: NodeId,
) -> Vec<(NodeId, NodeId, Overrides)> {
    let mut out = Vec::new();
    for (old_root, link) in &old.design.component_links {
        if *old_root != root && !old.is_ancestor(root, *old_root) {
            continue;
        }
        let Some(new_link) = new.design.component_links.get(old_root) else {
            continue;
        };
        if new_link.component != link.component || new_link.variant != link.variant {
            continue;
        }
        for (key, flags) in &link.overrides {
            if let (Some(before), Some(after)) = (link.members.get(key), new_link.members.get(key))
            {
                out.push((*before, *after, *flags));
            }
        }
    }
    out
}
pub(super) fn copy_group(
    plan: &mut Plan,
    source: &Document,
    root: NodeId,
    target: Option<NodeId>,
    hidden: bool,
    matches: &BTreeMap<NodeId, NodeId>,
    preserve: bool,
) -> Result<(NodeId, BTreeMap<NodeId, NodeId>), String> {
    if !source.node(root).is_some_and(|n| n.is_group()) {
        return Err("Component source is not a group".into());
    }
    if let Some(target) = target {
        editable(&plan.doc, target)?;
        if plan
            .doc
            .node(target)
            .is_some_and(|n| n.link_group.is_some())
        {
            return Err("Unlink instance movement links before updating.".into());
        }
    }
    let before = plan.doc.clone();
    let ids: HashSet<_> = source.subtree(root).into_iter().collect();
    let removed: HashSet<_> = target
        .map(|id| plan.doc.subtree(id).into_iter().collect())
        .unwrap_or_default();
    let mut map: BTreeMap<_, _> = matches
        .iter()
        .filter(|(a, b)| ids.contains(a) && removed.contains(b))
        .map(|(a, b)| (*a, *b))
        .collect();
    let dest = target.unwrap_or_else(|| plan.doc.alloc_id());
    map.insert(root, dest);
    // Match nested members by their own definition IDs even if the parent did
    // not previously contain those newly added members.
    for _ in 0..32 {
        let previous = map.len();
        for (nested, link) in &source.design.component_links {
            if *nested == root || !ids.contains(nested) {
                continue;
            }
            let Some(to) = map.get(nested).copied() else {
                continue;
            };
            let Some(old) = before.design.component_links.get(&to) else {
                continue;
            };
            if old.component == link.component && old.variant == link.variant {
                for (key, from) in &link.members {
                    if let Some(to) = old.members.get(key)
                        && removed.contains(to)
                    {
                        map.insert(*from, *to);
                    }
                }
            }
        }
        if previous == map.len() {
            break;
        }
    }
    for node in &source.nodes {
        if ids.contains(&node.id) && !map.contains_key(&node.id) {
            map.insert(node.id, plan.doc.alloc_id());
        }
    }
    let hash: HashMap<_, _> = map.iter().map(|(a, b)| (*a, *b)).collect();
    let previous_root = target.and_then(|id| before.node(id)).cloned();
    let position = target
        .and_then(|id| plan.doc.nodes.iter().position(|n| n.id == id))
        .unwrap_or(plan.doc.nodes.len());
    let mut copies = Vec::new();
    for original in source.nodes.iter().filter(|n| ids.contains(&n.id)) {
        let mut node = original.clone();
        node.id = map[&original.id];
        node.link_group = None;
        node.locked = false;
        node.locks = Default::default();
        node.parent = if original.id == root {
            previous_root.as_ref().and_then(|n| n.parent)
        } else {
            original.parent.map(|id| map[&id])
        };
        node.clip_to = original.clip_to.and_then(|id| map.get(&id).copied());
        if original.id == root {
            node.visible = !hidden;
            if let Some(old) = &previous_root {
                node.name = old.name.clone();
                node.clip_to = old.clip_to;
                node.visible = !hidden;
            } else if hidden {
                node.name = format!("Component source · {}", node.name);
            }
        }
        overrides::refresh(&mut node, plan.doc.width, plan.doc.height);
        copies.push(node);
    }
    plan.doc.nodes.retain(|n| !removed.contains(&n.id));
    let insert = before
        .nodes
        .iter()
        .take(position)
        .filter(|node| !removed.contains(&node.id))
        .count()
        .min(plan.doc.nodes.len());
    plan.doc.nodes.splice(insert..insert, copies);
    plan.doc.retain_raw_originals(source);
    let mut additions = source.design.fragment(&ids).remap(&hash);
    // Source definitions are imported separately. Nested instance metadata must
    // survive even though its hidden definition lies outside this subtree.
    additions.component_links = source
        .design
        .component_links
        .iter()
        .filter(|(id, _)| **id != root && ids.contains(id))
        .map(|(id, l)| (map[id], l.remap(&hash)))
        .collect();
    let mut design = plan.doc.design.clone();
    for id in &removed {
        design.component_links.remove(id);
        design.charts.remove(id);
        design.frames.remove(id);
        design.media.remove(id);
        design.constraints.remove(id);
        design.motion.remove(id);
        design.style_links.remove(id);
        design.variable_bindings.remove(id);
        design.data_bindings.remove(id);
        design.interactions.remove(id);
        design.interaction_triggers.remove(id);
        design.overlays.remove(id);
        design.local_media.remove(id);
        design.keyframes.remove(id);
    }
    if let Some(link) = before.design.component_links.get(&dest) {
        design.component_links.insert(dest, link.clone());
    }
    for (id, mut link) in std::mem::take(&mut additions.component_links) {
        if let Some(old) = before.design.component_links.get(&id)
            && old.component == link.component
        {
            link.auto_overrides = old.auto_overrides;
            if preserve && old.variant == link.variant {
                link.overrides = old.overrides.clone();
                link.overrides.retain(|k, _| link.members.contains_key(k));
            }
        }
        design.component_links.insert(id, link);
    }
    crate::design_variables::merge_into(&mut design, &additions);
    design.interactions.extend(additions.interactions.clone());
    design.interaction_triggers.extend(additions.interaction_triggers.clone());
    design.overlays.extend(additions.overlays.clone());
    design.local_media.extend(additions.local_media.clone());
    design.data_bindings.extend(additions.data_bindings.clone());
    design.fonts.extend(additions.fonts.clone());
    if !additions.keyframes.is_empty() {
        design.duration_ms = design.duration_ms.max(additions.duration_ms);
    }
    design.keyframes.extend(additions.keyframes.clone());
    crate::design_styles::merge_into(&mut design, &additions);
    design.charts.extend(additions.charts);
    design.frames.extend(additions.frames);
    design.media.extend(additions.media);
    design.constraints.extend(additions.constraints);
    for (id, motion) in additions.motion {
        design.duration_ms = design.duration_ms.max(motion.end_ms);
        design.motion.insert(id, motion);
    }
    if let Some(link) = design.component_links.get_mut(&dest) {
        link.members = map.clone();
    }
    plan.doc.design = design;
    if let Some(diagram) = &source.diagram {
        let additions = diagram.fragment(&ids).remap(&hash);
        let mut model = plan.doc.diagram.as_deref().cloned().unwrap_or_default();
        model.shapes.retain(|id, _| !removed.contains(id));
        model.edges.retain(|id, _| !removed.contains(id));
        model.shapes.extend(additions.shapes);
        model.edges.extend(additions.edges);
        plan.doc.diagram = Some(std::sync::Arc::new(model));
    }
    if preserve {
        for (old, new, flags) in preserve_flags(&before, &plan.doc, dest) {
            if let Some(node) = plan.doc.node_mut(new) {
                overrides::restore(before.node(old).unwrap(), node, flags, false)?;
            }
        }
    }
    plan.doc.normalize();
    Ok((dest, map))
}
pub(super) fn restore_geometry(
    plan: &mut Plan,
    before: &Document,
    root: NodeId,
    _map: &BTreeMap<NodeId, NodeId>,
    _link: &Instance,
) -> Result<(), String> {
    for (old, new, flags) in preserve_flags(before, &plan.doc, root) {
        let (w, h) = (plan.doc.width, plan.doc.height);
        if let Some(node) = plan.doc.node_mut(new) {
            overrides::restore(before.node(old).unwrap(), node, flags, true)?;
            overrides::refresh(node, w, h);
        }
    }
    Ok(())
}
pub(super) fn import_into(
    plan: &mut Plan,
    source: &Document,
    name: &str,
) -> Result<String, String> {
    source.validate().map_err(|e| e.to_string())?;
    if !source.design.components.contains_key(name) {
        return Err("Component no longer exists on that page".into());
    }
    let order = dependency_order(&source.design, source)?;
    let mut needed = HashSet::from([name.to_owned()]);
    for _ in 0..32 {
        let old = needed.len();
        for n in needed.clone() {
            needed.extend(dependencies(&source.design, source, &n));
        }
        if old == needed.len() {
            break;
        }
    }
    let mut renamed: BTreeMap<String, String> = BTreeMap::new();
    let mut definition_ids = HashMap::new();
    for original in order.into_iter().filter(|n| needed.contains(n)) {
        let stem: String = original.chars().take(64).collect();
        let mut chosen = original.clone();
        let mut suffix = 2;
        while plan.doc.design.components.contains_key(&chosen) {
            chosen = format!("{stem} ({suffix})");
            suffix += 1;
        }
        let definition = source.design.components[&original].clone();
        let mut variants = BTreeMap::new();
        let mut sources = HashMap::new();
        for (variant, root) in definition.variants.clone() {
            let (dest, map) = copy_group(plan, source, root, None, true, &BTreeMap::new(), false)?;
            for node in plan.doc.subtree(dest) {
                if let Some(link) = plan.doc.design.component_links.get_mut(&node) {
                    if let Some(name) = renamed.get(&link.component) {
                        link.component = name.clone();
                    }
                    link.members = link
                        .members
                        .iter()
                        .map(|(a, b)| (definition_ids.get(a).copied().unwrap_or(*a), *b))
                        .collect();
                    link.overrides = link
                        .overrides
                        .iter()
                        .map(|(a, b)| (definition_ids.get(a).copied().unwrap_or(*a), *b))
                        .collect();
                }
            }
            variants.insert(variant, dest);
            sources.extend(map);
        }
        plan.doc.design.components.insert(
            chosen.clone(),
            Definition {
                variants,
                library_id: definition.library_id,
                member_keys: definition
                    .member_keys
                    .into_iter()
                    .filter_map(|(id, key)| sources.get(&id).map(|to| (*to, key)))
                    .collect(),
            },
        );
        renamed.insert(original.clone(), chosen.clone());
        definition_ids.extend(sources);
    }
    Ok(renamed[name].clone())
}
