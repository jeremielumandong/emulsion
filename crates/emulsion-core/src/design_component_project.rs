//! Explicit cross-page publishing with stable component/member identities.
use super::*;
use crate::project::{PageId, ProjectEditor};

fn identity() -> String {
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{time:x}-{:x}-{serial:x}", std::process::id())
}
pub(super) fn refresh_keys(doc: &mut Document) {
    for name in doc.design.components.keys().cloned().collect::<Vec<_>>() {
        let ids: HashSet<_> = doc.design.components[&name]
            .variants
            .values()
            .flat_map(|root| doc.subtree(*root))
            .collect();
        let definition = doc.design.components.get_mut(&name).unwrap();
        if definition.library_id.is_empty() {
            definition.library_id = identity();
        }
        definition.member_keys.retain(|id, _| ids.contains(id));
        for id in ids {
            definition.member_keys.entry(id).or_insert_with(identity);
        }
    }
}

/// Copies imported from another page retain a family identity for explicit publishing.
pub fn insert_project(
    project: &mut ProjectEditor,
    source_page: PageId,
    name: &str,
    variant: &str,
    offset: (f64, f64),
) -> Result<NodeId, String> {
    let active = project.active_page();
    if source_page == active {
        return insert(project, name, variant, offset);
    }
    let mut source = project
        .page(source_page)
        .ok_or("Source page no longer exists.")?
        .doc
        .clone();
    refresh_keys(&mut source);
    let source_def = source
        .design
        .components
        .get(name)
        .ok_or("Component no longer exists.")?;
    let existing = project
        .doc
        .design
        .components
        .iter()
        .find(|(_, d)| d.library_id == source_def.library_id)
        .map(|(n, _)| n.clone());
    let mut target = Editor::new(project.doc.clone(), None);
    let id = if let Some(existing_name) = existing {
        if !target.doc.design.components[&existing_name]
            .variants
            .contains_key(variant)
        {
            let requested = HashSet::from([(name.to_owned(), variant.to_owned())]);
            synchronize_page(&mut target, &source, &requested)?;
        }
        insert(&mut target, &existing_name, variant, offset)?
    } else {
        import_and_insert(&mut target, &source, name, variant, offset)?
    };
    project.commit_documents(
        BTreeMap::from([(source_page, source), (active, target.doc)]),
        "Insert project component",
    )?;
    Ok(id)
}

/// Publish the active edited instance and refresh matching families on every page.
pub fn publish_project(project: &mut ProjectEditor, instance: NodeId) -> Result<usize, String> {
    let active = project.active_page();
    let mut source = Editor::new(project.doc.clone(), None);
    update(&mut source, instance, None)?;
    let link = source
        .doc
        .design
        .component_links
        .get(&instance)
        .ok_or("Select a linked component group")?;
    let mut affected = HashSet::from([(link.component.clone(), link.variant.clone())]);
    for name in nested::dependency_order(&source.doc.design, &source.doc)? {
        for (variant, root) in &source.doc.design.components[&name].variants {
            if source
                .doc
                .subtree(*root)
                .iter()
                .filter_map(|id| source.doc.design.component_links.get(id))
                .any(|link| affected.contains(&(link.component.clone(), link.variant.clone())))
            {
                affected.insert((name.clone(), variant.clone()));
            }
        }
    }
    let identities: HashSet<_> = affected
        .iter()
        .map(|(name, _)| source.doc.design.components[name].library_id.clone())
        .collect();
    let mut pages = BTreeMap::from([(active, source.doc.clone())]);
    for page in project.page_list() {
        if page.id == active {
            continue;
        }
        let original = &project.page(page.id).unwrap().doc;
        if !original
            .design
            .components
            .values()
            .any(|d| !d.library_id.is_empty() && identities.contains(&d.library_id))
        {
            continue;
        }
        let mut editor = Editor::new(original.clone(), None);
        let present: HashSet<_> = affected
            .iter()
            .filter(|(name, _)| {
                let family = &source.doc.design.components[name].library_id;
                original
                    .design
                    .components
                    .values()
                    .any(|d| d.library_id == *family)
            })
            .cloned()
            .collect();
        synchronize_page(&mut editor, &source.doc, &present)?;
        if editor.doc != *original {
            pages.insert(page.id, editor.doc);
        }
    }
    let count = pages.len();
    project.commit_documents(pages, "Publish component across project")?;
    Ok(count)
}

/// Copy selected variants, adding only missing dependencies. Existing variants
/// outside the publish graph are never overwritten or subjected to lock checks.
fn synchronize_page(
    editor: &mut Editor,
    source: &Document,
    requested: &HashSet<(String, String)>,
) -> Result<(), String> {
    let mut plan = Plan::new(editor)?;
    let order = nested::dependency_order(&source.design, source)?;
    let mut needed = requested.clone();
    // Dependencies already available on the destination supply canonical IDs;
    // only missing variants need artwork imported.
    for _ in 0..32 {
        let count = needed.len();
        for (name, variant) in needed.clone() {
            let root = *source
                .design
                .components
                .get(&name)
                .and_then(|d| d.variants.get(&variant))
                .ok_or("Component variant no longer exists.")?;
            for id in source.subtree(root) {
                let Some(link) = source.design.component_links.get(&id) else {
                    continue;
                };
                let family = &source.design.components[&link.component].library_id;
                let available = plan
                    .doc
                    .design
                    .components
                    .values()
                    .find(|d| d.library_id == *family)
                    .is_some_and(|d| d.variants.contains_key(&link.variant));
                if !available {
                    needed.insert((link.component.clone(), link.variant.clone()));
                }
            }
        }
        if needed.len() == count {
            break;
        }
    }
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut definition_ids: HashMap<NodeId, NodeId> = HashMap::new();
    let mut updated = HashSet::new();
    for original in order {
        let definition = &source.design.components[&original];
        let mut aliases: Vec<_> = plan
            .doc
            .design
            .components
            .iter()
            .filter(|(_, d)| d.library_id == definition.library_id)
            .map(|(name, _)| name.clone())
            .collect();
        let selected: Vec<_> = definition
            .variants
            .iter()
            .filter(|(variant, _)| needed.contains(&(original.clone(), (*variant).clone())))
            .map(|(variant, root)| (variant.clone(), *root))
            .collect();
        if aliases.is_empty() && selected.is_empty() {
            continue;
        }
        if aliases.is_empty() {
            let stem: String = original.chars().take(64).collect();
            let mut name = original.clone();
            let mut i = 2;
            while plan.doc.design.components.contains_key(&name) {
                name = format!("{stem} ({i})");
                i += 1;
            }
            aliases.push(name);
        }
        // A single canonical local definition resolves nested source IDs. Other
        // aliases retain their own member identities and are refreshed as well.
        let canonical = aliases[0].clone();
        for chosen in aliases {
            let previous = plan
                .doc
                .design
                .components
                .get(&chosen)
                .cloned()
                .unwrap_or_default();
            let old_keys: BTreeMap<_, _> = previous
                .member_keys
                .iter()
                .map(|(id, key)| (key, *id))
                .collect();
            let matches: BTreeMap<_, _> = definition
                .member_keys
                .iter()
                .filter_map(|(id, key)| old_keys.get(key).map(|to| (*id, *to)))
                .collect();
            if chosen == canonical {
                definition_ids.extend(matches.clone());
            }
            if selected.is_empty() {
                continue;
            }
            let mut variants = previous.variants;
            let mut member_keys = previous.member_keys;
            for (variant, root) in &selected {
                let target = variants.get(variant).copied();
                if target.is_some() && !requested.contains(&(original.clone(), variant.clone())) {
                    continue;
                }
                let (dest, map) =
                    copy_group(&mut plan, source, *root, target, true, &matches, false)?;
                for id in plan.doc.subtree(dest) {
                    if let Some(link) = plan.doc.design.component_links.get_mut(&id) {
                        if let Some(name) = names.get(&link.component) {
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
                variants.insert(variant.clone(), dest);
                member_keys.extend(
                    definition
                        .member_keys
                        .iter()
                        .filter_map(|(id, key)| map.get(id).map(|to| (*to, key.clone()))),
                );
                if chosen == canonical {
                    definition_ids.extend(map);
                }
                updated.insert((chosen.clone(), variant.clone()));
            }
            plan.doc.design.components.insert(
                chosen,
                Definition {
                    variants,
                    library_id: definition.library_id.clone(),
                    member_keys,
                },
            );
        }
        names.insert(original, canonical);
    }
    nested::prune_members(&mut plan.doc);
    // Also refresh destination-only wrappers that actually contain an affected
    // variant, without touching their unrelated variants or sibling families.
    for name in nested::dependency_order(&plan.doc.design, &plan.doc)? {
        let definition = plan.doc.design.components[&name].clone();
        for (variant, source) in definition.variants {
            let key = (name.clone(), variant.clone());
            let depends = plan
                .doc
                .subtree(source)
                .iter()
                .filter_map(|id| plan.doc.design.component_links.get(id))
                .any(|link| updated.contains(&(link.component.clone(), link.variant.clone())));
            if !updated.contains(&key) && !depends {
                continue;
            }
            updated.insert(key);
            let targets: Vec<_> = plan
                .doc
                .design
                .component_links
                .iter()
                .filter(|(_, l)| l.component == name && l.variant == variant)
                .map(|(id, _)| *id)
                .collect();
            for target in targets {
                replace_instance(&mut plan, source, target, true)?;
            }
        }
    }
    plan.commit(editor, "Refresh project components")
}

#[cfg(test)]
#[path = "design_component_project_tests.rs"]
mod tests;
