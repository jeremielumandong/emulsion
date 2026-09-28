//! Run against an external draw.io sample checkout; emit one JSON record per file.
use rayon::prelude::*;
use std::path::Path;

fn visit(path: &Path, files: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_name() == ".git" || entry.file_type()?.is_symlink() {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            visit(&path, files)?;
        } else if path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
            matches!(
                s.to_ascii_lowercase().as_str(),
                "drawio"
                    | "xml"
                    | "svg"
                    | "vsdx"
                    | "vssx"
                    | "vstx"
                    | "vssm"
                    | "vstm"
                    | "vsd"
                    | "vss"
                    | "vst"
                    | "vsx"
                    | "vdx"
            )
        }) {
            files.push(path);
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args().nth(1).ok_or("Pass a sample directory")?;
    let root = Path::new(&root);
    let mut files = Vec::new();
    visit(root, &mut files)?;
    files.sort();
    if let Ok(report) = std::env::var("EMULSION_AUDIT_RETRY_JSONL") {
        let rows = std::fs::read_to_string(report)?;
        let failed = rows
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|v| v.get("error").is_some())
            .filter_map(|v| v["file"].as_str().map(|s| root.join(s)))
            .collect::<std::collections::HashSet<_>>();
        files.retain(|p| failed.contains(p));
    }
    let exercise = std::env::args().any(|a| a == "--exercise");
    let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build()?;
    pool.install(|| files.par_iter().for_each(|path| {
        let start = std::time::Instant::now();
        let result = emulsion_io::diagram_import::read(&path);
        let mut row = serde_json::json!({"file": path.strip_prefix(root).unwrap(), "ms": start.elapsed().as_millis()});
        match result {
            Ok(imported) => {
                row["pages"] = imported.project.pages.len().into();
                row["shapes"] = imported
                    .project
                    .pages
                    .iter()
                    .map(|p| {
                        p.doc.diagram.as_ref().map_or(0, |d| {
                            d.shapes
                                .values()
                                .filter(|s| !s.data.contains_key("emulsion_drawio_endpoint"))
                                .count()
                        })
                    })
                    .sum::<usize>()
                    .into();
                row["edges"] = imported
                    .project
                    .pages
                    .iter()
                    .map(|p| p.doc.diagram.as_ref().map_or(0, |d| d.edges.len()))
                    .sum::<usize>()
                    .into();
                row["warnings"] = serde_json::to_value(imported.warnings).unwrap();
                if exercise {
                    let result = (|| -> Result<(),String> {
                        for page in &imported.project.pages {
                            let Some(model) = &page.doc.diagram else { continue; };
                            let selected = model.edges.values().next().map(|e|e.source.shape).or_else(||model.shapes.keys().next().copied());
                            if let Some(id) = selected {
                                let before = page.doc.clone();
                                let mut editor = emulsion_core::Editor::new(before.clone(),None);
                                editor.execute(emulsion_core::Command::TranslateNode{id,dx:13.,dy:7.}).map_err(|e|e.to_string())?;
                                editor.doc.validate().map_err(|e|e.to_string())?;
                                editor.undo();
                                if editor.doc != before { return Err("Movement undo changed source document".into()); }
                            }
                        }
                        Ok(())
                    })();
                    match result { Ok(())=>row["movement_undo"]="passed".into(), Err(e)=>row["exercise_error"]=e.into() }
                }
            }
            Err(e) => row["error"] = e.to_string().into(),
        }
        println!("{row}");
    }));
    Ok(())
}
