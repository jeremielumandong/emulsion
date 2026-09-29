//! Deterministic local layouts. Cycles and disconnected components remain visible.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Vertical,
    Horizontal,
    Grid,
    MindMap,
}
impl Layout {
    pub const ALL: [Self; 4] = [Self::Vertical, Self::Horizontal, Self::Grid, Self::MindMap];
    pub fn label(self) -> &'static str {
        match self {
            Self::Vertical => "Top to bottom",
            Self::Horizontal => "Left to right",
            Self::Grid => "Grid",
            Self::MindMap => "Mind map",
        }
    }
}
pub fn arrange(editor: &mut Editor, layout: Layout) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    let model = editor.doc.diagram.as_deref().ok_or("Add shapes first")?;
    // Containers move their children as one unit; individual children retain their layout.
    let shapes = model
        .shapes
        .iter()
        .filter(|(id, s)| {
            s.container.is_none()
                && !s.layout_locked
                && editor.doc.locked_ancestor(**id).is_none()
                && !editor.doc.layer_locks(**id).position
        })
        .map(|(id, s)| (*id, shape_bounds(&editor.doc, s).unwrap()))
        .collect::<BTreeMap<_, _>>();
    if shapes.is_empty() {
        return Err("No unlocked shapes to arrange.".into());
    }
    let mut incoming: HashMap<NodeId, usize> = shapes.keys().map(|id| (*id, 0)).collect();
    let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for edge in model.edges.values() {
        let (a, b) = (edge.source.shape, edge.target.shape);
        if a != b && shapes.contains_key(&a) && shapes.contains_key(&b) {
            *incoming.get_mut(&b).unwrap() += 1;
            adjacency.entry(a).or_default().push(b);
        }
    }
    let mut ranks: HashMap<NodeId, usize> = HashMap::new();
    let mut queue = std::collections::VecDeque::new();
    for (id, degree) in &incoming {
        if *degree == 0 {
            queue.push_back(*id);
            ranks.insert(*id, 0);
        }
    }
    queue.make_contiguous().sort();
    while let Some(id) = queue.pop_front() {
        for next in adjacency.get(&id).into_iter().flatten() {
            let rank = ranks[&id] + 1;
            ranks
                .entry(*next)
                .and_modify(|r| *r = (*r).max(rank))
                .or_insert(rank);
            let degree = incoming.get_mut(next).unwrap();
            *degree -= 1;
            if *degree == 0 {
                queue.push_back(*next);
            }
        }
    }
    // Cycles cannot be topologically ordered. Put each remaining component in
    // deterministic breadth-first levels, never recurse through a cycle.
    for id in shapes.keys() {
        if ranks.contains_key(id) {
            continue;
        }
        ranks.insert(*id, 0);
        let mut pending = std::collections::VecDeque::from([*id]);
        while let Some(id) = pending.pop_front() {
            for next in adjacency.get(&id).into_iter().flatten() {
                if !ranks.contains_key(next) {
                    ranks.insert(*next, ranks[&id] + 1);
                    pending.push_back(*next);
                }
            }
        }
    }
    let width = shapes.values().map(|b| b[2]).fold(1., f64::max) + 80.;
    let height = shapes.values().map(|b| b[3]).fold(1., f64::max) + 60.;
    let columns = (shapes.len() as f64).sqrt().ceil() as usize;
    let mut rows: HashMap<usize, usize> = HashMap::new();
    // Geometry is staged without the graph, then every connection is derived
    // once at the end. The transaction and preflight keep this atomic.
    let saved_model = editor.doc.diagram.clone();
    let mut commands = vec![Command::SetDiagram { diagram: None }];
    let mut occupied = model
        .shapes
        .iter()
        .filter(|(id, s)| s.container.is_none() && !shapes.contains_key(id))
        .filter_map(|(_, s)| shape_bounds(&editor.doc, s))
        .collect::<Vec<_>>();
    for (index, (id, bounds)) in shapes.iter().enumerate() {
        let rank = ranks[id];
        let row = rows.entry(rank).or_default();
        let (mut x, mut y) = match layout {
            Layout::Grid => (
                40. + (index % columns) as f64 * width,
                40. + (index / columns) as f64 * height,
            ),
            Layout::Vertical => (40. + *row as f64 * width, 40. + rank as f64 * height),
            Layout::Horizontal => (40. + rank as f64 * width, 40. + *row as f64 * height),
            Layout::MindMap => {
                let angle = (index as f64 / shapes.len().max(1) as f64) * std::f64::consts::TAU;
                let radius = (rank + 1) as f64 * width;
                (
                    (editor.doc.width as f64 - bounds[2]) / 2. + angle.cos() * radius,
                    (editor.doc.height as f64 - bounds[3]) / 2. + angle.sin() * radius,
                )
            }
        };
        *row += 1;
        x = x.max(40.);
        y = y.max(40.);
        // Respect manually fixed shapes and already placed objects. If a page
        // cannot contain the layout, leave overflow visible for the UI to report.
        for _ in 0..=model.shapes.len() {
            let collision = occupied.iter().find(|r| {
                x < r[0] + r[2] + 20.
                    && x + bounds[2] + 20. > r[0]
                    && y < r[1] + r[3] + 20.
                    && y + bounds[3] + 20. > r[1]
            });
            let Some(r) = collision else {
                break;
            };
            x = r[0] + r[2] + 40.;
            if x + bounds[2] > editor.doc.width as f64 {
                x = 40.;
                y = r[1] + r[3] + 40.;
            }
        }
        if occupied.iter().any(|r| {
            x < r[0] + r[2] + 20.
                && x + bounds[2] + 20. > r[0]
                && y < r[1] + r[3] + 20.
                && y + bounds[3] + 20. > r[1]
        }) {
            return Err("Could not arrange these shapes without overlapping fixed objects. Move or unlock them and try again.".into());
        }
        occupied.push([x, y, bounds[2], bounds[3]]);
        commands.push(Command::TranslateNode {
            id: *id,
            dx: x - bounds[0],
            dy: y - bounds[1],
        });
    }
    commands.push(Command::SetDiagram {
        diagram: saved_model,
    });
    // Validate all moves first, including dependent locked connectors.
    let mut trial = editor.doc.clone();
    for command in &commands {
        command.apply(&mut trial).map_err(|e| e.to_string())?;
    }
    editor.begin("Arrange diagram");
    for command in commands {
        if let Err(e) = editor.execute(command) {
            editor.cancel();
            return Err(e.to_string());
        }
    }
    editor.end();
    Ok(())
}
