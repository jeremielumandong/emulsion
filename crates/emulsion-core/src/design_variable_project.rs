//! Explicit project-wide variable libraries; every operation is one grouped Undo.
use crate::{
    Document, Editor, design_variables as variables,
    project::{PageId, ProjectEditor},
};
use std::collections::BTreeMap;

fn identity() -> String {
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!(
        "{:x}-{:x}-{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
fn documents(project: &ProjectEditor) -> Result<BTreeMap<PageId, Document>, String> {
    if project.kind().is_none() {
        return Err("Open a Design or Diagram project first.".into());
    }
    Ok(project
        .page_list()
        .iter()
        .map(|p| (p.id, project.page(p.id).unwrap().doc.clone()))
        .collect())
}
/// Share an existing local variable with every page. Existing local names are
/// never silently adopted; resolve collisions or import with a different name.
pub fn share(project: &mut ProjectEditor, name: &str) -> Result<usize, String> {
    if project.doc.design.variable_libraries.contains_key(name) {
        return publish(project, name);
    }
    let value = project
        .doc
        .design
        .variables
        .get(name)
        .cloned()
        .ok_or("Unknown variable.")?;
    let active = project.active_page();
    let family = identity();
    let mut docs = documents(project)?;
    for (page, doc) in &mut docs {
        if *page != active && doc.design.variables.contains_key(name) {
            return Err(format!(
                "Page {page} already has a local variable named {name}. Rename it or import the variable under another name."
            ));
        }
        doc.design.variables.insert(name.into(), value.clone());
        doc.design
            .variable_libraries
            .insert(name.into(), family.clone());
    }
    let count = docs.len();
    project.commit_documents(docs, "Share project variable")?;
    Ok(count)
}
/// Import a variable from another page. Local names may differ while the stable
/// library identity ensures later publication reaches the correct consumers.
pub fn import(
    project: &mut ProjectEditor,
    source: PageId,
    name: &str,
    target_name: &str,
) -> Result<(), String> {
    let active = project.active_page();
    if source == active {
        return Err("Choose another source page.".into());
    }
    let mut docs = documents(project)?;
    let source_doc = docs
        .get_mut(&source)
        .ok_or("Source page no longer exists.")?;
    let value = source_doc
        .design
        .variables
        .get(name)
        .cloned()
        .ok_or("Source variable no longer exists.")?;
    let family = source_doc
        .design
        .variable_libraries
        .entry(name.into())
        .or_insert_with(identity)
        .clone();
    let target = docs.get_mut(&active).unwrap();
    if target.design.variables.contains_key(target_name) {
        return Err("The target variable name already exists.".into());
    }
    let mut trial = Editor::new(target.clone(), None);
    variables::set(&mut trial, target_name, value)?;
    trial
        .doc
        .design
        .variable_libraries
        .insert(target_name.into(), family);
    docs.insert(active, trial.doc);
    project.commit_documents(docs, "Import project variable")?;
    Ok(())
}
pub fn publish(project: &mut ProjectEditor, name: &str) -> Result<usize, String> {
    let family = project
        .doc
        .design
        .variable_libraries
        .get(name)
        .cloned()
        .ok_or("Share or import this variable first.")?;
    let value = project
        .doc
        .design
        .variables
        .get(name)
        .cloned()
        .ok_or("Unknown variable.")?;
    let mut docs = documents(project)?;
    let mut count = 0;
    for doc in docs.values_mut() {
        let names: Vec<_> = doc
            .design
            .variable_libraries
            .iter()
            .filter(|(_, id)| **id == family)
            .map(|(name, _)| name.clone())
            .collect();
        if names.is_empty() {
            continue;
        }
        count += 1;
        let mut trial = Editor::new(doc.clone(), None);
        for name in names {
            variables::set(&mut trial, &name, value.clone())?;
        }
        *doc = trial.doc;
    }
    project.commit_documents(docs, "Publish project variable")?;
    Ok(count)
}
pub fn rename(project: &mut ProjectEditor, name: &str, new_name: &str) -> Result<(), String> {
    let family = project
        .doc
        .design
        .variable_libraries
        .get(name)
        .cloned()
        .ok_or("Share or import this variable first.")?;
    let mut docs = documents(project)?;
    for doc in docs.values_mut() {
        let names: Vec<_> = doc
            .design
            .variable_libraries
            .iter()
            .filter(|(_, id)| **id == family)
            .map(|(name, _)| name.clone())
            .collect();
        if names.len() > 1 {
            return Err("A page has multiple aliases of this variable. Detach extra aliases before renaming the library.".into());
        }
        let mut trial = Editor::new(doc.clone(), None);
        for name in names {
            variables::rename(&mut trial, &name, new_name)?;
        }
        *doc = trial.doc;
    }
    project.commit_documents(docs, "Rename project variable")?;
    Ok(())
}
pub fn remove(project: &mut ProjectEditor, name: &str) -> Result<(), String> {
    let family = project
        .doc
        .design
        .variable_libraries
        .get(name)
        .cloned()
        .ok_or("Share or import this variable first.")?;
    let mut docs = documents(project)?;
    for doc in docs.values_mut() {
        let names: Vec<_> = doc
            .design
            .variable_libraries
            .iter()
            .filter(|(_, id)| **id == family)
            .map(|(name, _)| name.clone())
            .collect();
        let mut trial = Editor::new(doc.clone(), None);
        for name in names {
            variables::remove(&mut trial, &name)?;
        }
        *doc = trial.doc;
    }
    project.commit_documents(docs, "Remove project variable")?;
    Ok(())
}
pub fn detach(project: &mut ProjectEditor, name: &str) -> Result<(), String> {
    let mut doc = project.doc.clone();
    if doc.design.variable_libraries.remove(name).is_none() {
        return Err("This variable is already local.".into());
    }
    project.commit_documents(
        BTreeMap::from([(project.active_page(), doc)]),
        "Detach project variable",
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Node, command::Slot, project::ProjectKind};
    #[test]
    fn project_variables_publish_aliases_locks_collisions_and_grouped_undo() {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(100, 100)).unwrap();
        let a = p.active_page();
        variables::set(&mut p, "Accent", variables::Value::Color([255, 0, 0, 255])).unwrap();
        p.add_page(Document::new(100, 100), "Second".into(), 0.)
            .unwrap();
        let b = p.active_page();
        import(&mut p, a, "Accent", "Highlight").unwrap();
        let family = p.doc.design.variable_libraries["Highlight"].clone();
        assert_eq!(
            p.page(a).unwrap().doc.design.variable_libraries["Accent"],
            family
        );
        let node = p
            .execute(Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    "Shape",
                    crate::NodeKind::Fill {
                        rgba: [0, 0, 0, 255],
                    },
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        variables::bind(
            &mut p,
            &[node],
            variables::Property::Fill,
            Some("Highlight"),
        )
        .unwrap();
        p.set_active_page(a).unwrap();
        variables::set(&mut p, "Accent", variables::Value::Color([0, 0, 255, 128])).unwrap();
        let before_a = p.doc.clone();
        let before_b = p.page(b).unwrap().doc.clone();
        assert_eq!(publish(&mut p, "Accent").unwrap(), 2);
        assert_eq!(
            p.page(b).unwrap().doc.design.variables["Highlight"],
            variables::Value::Color([0, 0, 255, 128])
        );
        p.undo();
        assert_eq!(p.page(a).unwrap().doc, before_a);
        assert_eq!(p.page(b).unwrap().doc, before_b);
        p.redo();
        // History activates the changed destination; select the alias being renamed.
        p.set_active_page(a).unwrap();
        rename(&mut p, "Accent", "Primary").unwrap();
        assert!(
            p.page(b)
                .unwrap()
                .doc
                .design
                .variables
                .contains_key("Primary")
        );
        remove(&mut p, "Primary").unwrap();
        assert!(p.page(b).unwrap().doc.design.variable_bindings.is_empty());
        p.undo();
        p.set_active_page(b).unwrap();
        detach(&mut p, "Primary").unwrap();
        assert!(p.doc.design.variable_libraries.is_empty());
        assert!(!p.doc.design.variable_bindings.is_empty());
    }
    #[test]
    fn project_variable_locked_consumer_and_name_collision_are_atomic() {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(100, 100)).unwrap();
        let a = p.active_page();
        variables::set(&mut p, "Alpha", variables::Value::Number(0.8)).unwrap();
        p.add_page(Document::new(100, 100), "Second".into(), 0.)
            .unwrap();
        let b = p.active_page();
        variables::set(&mut p, "Alpha", variables::Value::Number(0.3)).unwrap();
        p.set_active_page(a).unwrap();
        let before = p.doc.clone();
        let other = p.page(b).unwrap().doc.clone();
        assert!(share(&mut p, "Alpha").is_err());
        assert_eq!(p.doc, before);
        assert_eq!(p.page(b).unwrap().doc, other);
        p.set_active_page(b).unwrap();
        import(&mut p, a, "Alpha", "Shared").unwrap();
        let id = p
            .execute(Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    "Shape",
                    crate::NodeKind::Fill {
                        rgba: [0, 0, 0, 255],
                    },
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        variables::bind(&mut p, &[id], variables::Property::Opacity, Some("Shared")).unwrap();
        p.execute(Command::SetLocked { id, locked: true }).unwrap();
        p.set_active_page(a).unwrap();
        variables::set(&mut p, "Alpha", variables::Value::Number(0.2)).unwrap();
        let before = p.doc.clone();
        let other = p.page(b).unwrap().doc.clone();
        let revision = p.revision;
        assert!(publish(&mut p, "Alpha").unwrap_err().contains("Unlock"));
        assert_eq!(p.doc, before);
        assert_eq!(p.page(b).unwrap().doc, other);
        assert_eq!(p.revision, revision);
    }
}
