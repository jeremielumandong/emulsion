//! The revision graph of synchronized files. Revisions are immutable and
//! name their parent (and, for a merge, the head they merged), so every
//! device can work out the same heads and common ancestors from a listing,
//! whatever order it arrives in and without trusting device clocks.
use crate::{RemoteRevision, Revision};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Keep every concurrent head, independent of timestamps and listing order.
/// A merge revision supersedes both of its parents.
pub fn heads(revisions: &[RemoteRevision]) -> Vec<RemoteRevision> {
    let superseded: HashSet<(&str, &str)> = revisions
        .iter()
        .flat_map(|r| {
            r.revision
                .parents()
                .map(move |p| (r.revision.project.as_str(), p.as_str()))
        })
        .collect();
    revisions
        .iter()
        .filter(|r| !superseded.contains(&(r.revision.project.as_str(), r.revision.id.as_str())))
        .cloned()
        .collect()
}

fn by_id<'a>(revisions: impl IntoIterator<Item = &'a Revision>) -> HashMap<&'a str, &'a Revision> {
    revisions.into_iter().map(|r| (r.id.as_str(), r)).collect()
}

/// `id` and every revision it descends from that `revisions` knows.
pub fn ancestors<'a>(
    revisions: impl IntoIterator<Item = &'a Revision>,
    id: &str,
) -> HashSet<String> {
    let known = by_id(revisions);
    let mut out = HashSet::new();
    let mut stack = vec![id.to_string()];
    while let Some(next) = stack.pop() {
        if !out.insert(next.clone()) {
            continue;
        }
        if let Some(r) = known.get(next.as_str()) {
            stack.extend(r.parents().cloned());
        }
    }
    out
}

/// The lowest common ancestor of `a` and `b`: a shared ancestor that no
/// other shared ancestor descends from. With several (criss-cross merges)
/// the newest, then the greatest ID, wins, so every device picks the same.
pub fn merge_base<'a>(
    revisions: impl IntoIterator<Item = &'a Revision> + Clone,
    a: &str,
    b: &str,
) -> Option<String> {
    let known = by_id(revisions.clone());
    let left = ancestors(revisions.clone(), a);
    let right = ancestors(revisions.clone(), b);
    let common: HashSet<&String> = left
        .intersection(&right)
        .filter(|id| known.contains_key(id.as_str()))
        .collect();
    let lowest: Vec<&String> = common
        .iter()
        .copied()
        .filter(|c| {
            !common
                .iter()
                .any(|other| other != c && ancestors(revisions.clone(), other).contains(*c))
        })
        .collect();
    lowest
        .into_iter()
        .max_by_key(|id| (known[id.as_str()].created, (*id).clone()))
        .cloned()
}

/// The heads of `project` that `local` (the revision a local file was last
/// saved as) does not already include: other artists' saves waiting to be
/// merged, newest first.
pub fn waiting_heads(
    revisions: &[RemoteRevision],
    project: &str,
    local: Option<&str>,
) -> Vec<RemoteRevision> {
    let mine: Vec<&Revision> = revisions.iter().map(|r| &r.revision).collect();
    let included = local
        .map(|id| ancestors(mine.iter().copied(), id))
        .unwrap_or_default();
    let mut out: Vec<RemoteRevision> = heads(revisions)
        .into_iter()
        .filter(|r| r.revision.project == project && !included.contains(&r.revision.id))
        .collect();
    out.sort_by(|a, b| {
        (b.revision.created, &b.revision.id).cmp(&(a.revision.created, &a.revision.id))
    });
    out
}

/// Someone who saved revisions of a project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collaborator {
    pub device: String,
    /// Their latest author name, when their revisions carry one.
    pub author: Option<String>,
    pub revisions: usize,
    /// When their latest revision was made (their device's clock).
    pub last: u64,
}

/// Everyone who saved revisions of `project`, most recent first.
pub fn collaborators(revisions: &[RemoteRevision], project: &str) -> Vec<Collaborator> {
    let mut by_device: BTreeMap<&str, Collaborator> = BTreeMap::new();
    for r in revisions
        .iter()
        .map(|r| &r.revision)
        .filter(|r| r.project == project)
    {
        let entry = by_device
            .entry(r.device.as_str())
            .or_insert_with(|| Collaborator {
                device: r.device.clone(),
                author: None,
                revisions: 0,
                last: 0,
            });
        entry.revisions += 1;
        if r.created >= entry.last {
            entry.last = r.created;
            if r.author.is_some() {
                entry.author = r.author.clone();
            }
        }
        if entry.author.is_none() {
            entry.author = r.author.clone();
        }
    }
    let mut out: Vec<_> = by_device.into_values().collect();
    out.sort_by(|a, b| (b.last, &a.device).cmp(&(a.last, &b.device)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;

    fn rev(
        project: &str,
        parent: Option<&Revision>,
        merged: Option<&Revision>,
        created: u64,
    ) -> Revision {
        Revision {
            project: project.into(),
            id: id(),
            parent: parent.map(|p| p.id.clone()),
            hash: "a".repeat(64),
            name: "board.emu".into(),
            created,
            device: id(),
            bytes: 10,
            home: None,
            merged: merged.map(|m| m.id.clone()),
            author: Some("Maya".into()),
        }
    }
    fn rows(list: &[&Revision]) -> Vec<RemoteRevision> {
        list.iter()
            .map(|r| RemoteRevision {
                remote_id: r.id.clone(),
                revision: (*r).clone(),
            })
            .collect()
    }

    #[test]
    fn a_merge_revision_supersedes_both_heads_and_finds_the_common_ancestor() {
        let project = id();
        let base = rev(&project, None, None, 1);
        let left = rev(&project, Some(&base), None, 2);
        let right = rev(&project, Some(&base), None, 3);
        let two = rows(&[&base, &left, &right]);
        assert_eq!(heads(&two).len(), 2);
        let all = [&base, &left, &right];
        assert_eq!(merge_base(all, &left.id, &right.id), Some(base.id.clone()));
        // From the left device, the right head is waiting; and the other way.
        let waiting = waiting_heads(&two, &project, Some(&left.id));
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].revision.id, right.id);
        assert!(waiting_heads(&two, &project, Some(&base.id)).len() == 2);

        let merge = rev(&project, Some(&left), Some(&right), 4);
        merge.validate().unwrap();
        let three = rows(&[&base, &left, &right, &merge]);
        let h = heads(&three);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].revision.id, merge.id);
        assert!(waiting_heads(&three, &project, Some(&merge.id)).is_empty());
        // The right device sees the merge as including its own work.
        let from_right = waiting_heads(&three, &project, Some(&right.id));
        assert_eq!(from_right[0].revision.id, merge.id);
        let all = [&base, &left, &right, &merge];
        assert_eq!(
            merge_base(all, &merge.id, &right.id),
            Some(right.id.clone())
        );
        assert!(ancestors(all, &merge.id).contains(&base.id));

        // Criss-cross: two merges of the same heads; every device picks one.
        let other = rev(&project, Some(&right), Some(&left), 4);
        let all = [&base, &left, &right, &merge, &other];
        let chosen = merge_base(all, &merge.id, &other.id).unwrap();
        assert!(chosen == left.id || chosen == right.id);
        assert_eq!(
            merge_base([&other, &merge, &right, &left, &base], &merge.id, &other.id),
            Some(chosen)
        );
        assert_eq!(collaborators(&three, &project).len(), 4);
    }

    #[test]
    fn merge_headers_validate_and_stay_readable_by_old_readers() {
        let project = id();
        let base = rev(&project, None, None, 1);
        let mut merge = rev(&project, Some(&base), Some(&base), 2);
        assert!(merge.validate().is_err(), "two different parents");
        merge.merged = Some(id());
        merge.validate().unwrap();
        merge.parent = None;
        assert!(merge.validate().is_err(), "a merge has a first parent");
        merge.parent = Some(base.id.clone());
        merge.author = Some("bad\nname".into());
        assert!(merge.validate().is_err());
        merge.author = None;

        // Old revisions without the new fields still read.
        let mut legacy = serde_json::to_value(&base).unwrap();
        legacy.as_object_mut().unwrap().remove("merged");
        legacy.as_object_mut().unwrap().remove("author");
        let legacy: Revision = serde_json::from_value(legacy).unwrap();
        assert_eq!((legacy.merged, legacy.author), (None, None));
        // A reader that predates merge revisions ignores the second parent.
        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct OldRevision {
            project: String,
            id: String,
            parent: Option<String>,
            hash: String,
            name: String,
            created: u64,
            device: String,
            bytes: u64,
        }
        let old: OldRevision =
            serde_json::from_slice(&serde_json::to_vec(&merge).unwrap()).unwrap();
        assert_eq!(old.parent, Some(base.id.clone()));
        // Ordinary revisions do not write the new fields at all.
        let plain = serde_json::to_string(&Revision {
            author: None,
            ..base
        })
        .unwrap();
        assert!(!plain.contains("merged") && !plain.contains("author"));
    }
}
