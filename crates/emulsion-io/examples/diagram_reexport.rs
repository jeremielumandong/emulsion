//! Re-export a saved diagram without modifying its project or existing exports.
use emulsion_io::export::{ExportOptions, ExportScale, ExportWorkflow};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: diagram_reexport INPUT.emu OUTPUT.pdf OUTPUT.png".into());
    }
    let project = emulsion_io::project::read(std::path::Path::new(&args[1]))?;
    let doc = &project.pages[0].doc;
    println!(
        "Exporting {} × {}, {} nodes, diagram={}",
        doc.width,
        doc.height,
        doc.nodes.len(),
        doc.diagram.is_some()
    );
    emulsion_io::export::export(
        doc,
        std::path::Path::new(&args[2]),
        ExportOptions::for_doc(doc),
    )?;
    emulsion_io::export::export_with_workflow(
        doc,
        std::path::Path::new(&args[3]),
        ExportOptions::for_doc(doc),
        ExportWorkflow {
            scale: ExportScale::Quadruple,
            ..Default::default()
        },
    )?;
    Ok(())
}
