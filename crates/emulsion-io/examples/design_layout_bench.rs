//! Bounded, reproducible native Design layout workload; emits JSON measurements.
use emulsion_core::{
    Command, Document, Editor, Node,
    command::Slot,
    design_layout::{self, Child, Flow, Frame},
};
use emulsion_raster::{vector::PathStyle, vector_geometry::rectangle};
use std::{sync::Arc, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count = std::env::args()
        .nth(1)
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(1000)
        .clamp(10, 4000);
    let mut doc = Document::new(2400, 1800);
    let mut groups = Vec::new();
    for chunk in 0..count.div_ceil(100) {
        let group = Command::AddNode {
            node: Box::new(Node::group(0, format!("Section {chunk}"))),
            slot: Slot::TOP,
        }
        .apply(&mut doc)?
        .unwrap();
        groups.push(group);
        for i in chunk * 100..((chunk + 1) * 100).min(count) {
            Command::AddNode {
                node: Box::new(Node::path(
                    0,
                    format!("Card {i}"),
                    Arc::new(rectangle(
                        (i % 10) as f64 * 50.,
                        (i / 10) as f64 * 30.,
                        40.,
                        20.,
                    )),
                    PathStyle {
                        fill: Some([30, 90, 180, 255]),
                        stroke: None,
                        ..Default::default()
                    },
                    2400,
                    1800,
                )),
                slot: Slot::top_of(Some(group)),
            }
            .apply(&mut doc)?;
        }
    }
    let mut editor = Editor::new(doc, None);
    for group in &groups {
        let children = editor.doc.children(Some(*group));
        let frame = Frame {
            flow: Flow::Grid,
            columns: 10,
            padding: [8.; 4],
            gap: 4.,
            children: children
                .into_iter()
                .map(|id| {
                    (
                        id,
                        Child {
                            fill_width: true,
                            min_height: Some(20.),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
            ..Default::default()
        };
        design_layout::enable(&mut editor, *group, frame, (1200., 600.))?;
    }
    let mut samples = Vec::new();
    for i in 0..21 {
        let mut design = editor.doc.design.clone();
        design.frames.get_mut(&groups[0]).unwrap().gap = 4. + (i % 2) as f64;
        let start = Instant::now();
        editor.execute(Command::SetDesign {
            design: Box::new(design),
        })?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        if i > 0 {
            samples.push(elapsed);
        }
    }
    samples.sort_by(f64::total_cmp);
    let snapshot = editor.doc.clone();
    editor.execute(Command::SetDesign {
        design: Box::new(snapshot.design.clone()),
    })?;
    assert_eq!(editor.doc, snapshot, "Repeated reflow must be idempotent");
    editor.doc.validate()?;
    println!(
        "{}",
        serde_json::json!({"objects":count,"frames":groups.len(),"samples":samples.len(),"median_ms":samples[samples.len()/2],"p95_ms":samples[samples.len()*95/100],"max_ms":samples.last(),"idempotent":true})
    );
    Ok(())
}
