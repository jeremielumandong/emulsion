//! Run against an external draw.io sample checkout; emit one JSON record per file.
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
    for path in files {
        let start = std::time::Instant::now();
        let result = emulsion_io::diagram_import::read(&path);
        let mut row = serde_json::json!({"file": path.strip_prefix(root)?, "ms": start.elapsed().as_millis()});
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
                row["warnings"] = serde_json::to_value(imported.warnings)?;
            }
            Err(e) => row["error"] = e.to_string().into(),
        }
        println!("{row}");
    }
    Ok(())
}
