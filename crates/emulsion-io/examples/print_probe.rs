//! Read-only printer discovery and an optional PDF proof. Never sends a print job.
use emulsion_io::printing::{self as print, Layout, Placement, Settings, Source};
use std::{path::PathBuf, sync::atomic::AtomicBool};
fn main() -> anyhow::Result<()> {
    for printer in print::discover()? {
        println!("{}", serde_json::to_string(&printer)?);
        match print::capabilities(&printer.id) {
            Ok(caps) => {
                println!("{}", serde_json::to_string(&caps)?);
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                if let Some(paper) = caps.papers.iter().find(|p| p.id == caps.default_paper) {
                    print::validate_device_settings(
                        &printer.id,
                        &Settings {
                            paper: paper.clone(),
                            ..Default::default()
                        },
                    )?;
                    println!("Default device settings validated without submitting a job");
                }
            }
            Err(error) => eprintln!("{}: {error}", printer.name),
        }
    }
    if let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) {
        let sources = vec![Source {
            name: "100 mm proof".into(), width: 1000, height: 1000, ppi: 254., rasterized: false, original_paths: vec![],
            svg: "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"1000\"><rect x=\"5\" y=\"5\" width=\"990\" height=\"990\" fill=\"#f4e9d5\" stroke=\"#202020\" stroke-width=\"10\"/><circle cx=\"500\" cy=\"500\" r=\"200\" fill=\"#d93a1e\"/></svg>".into(),
        }];
        let settings = Settings {
            placement: Placement::Actual,
            layout: Layout::Single,
            ..Default::default()
        };
        let layout = print::layout(&sources, &[0], &settings)?;
        print::preview(&sources, &layout.sheets[0], false, 900)?
            .save(path.with_extension("png"))?;
        print::write_pdf(&sources, &layout, false, &path, &AtomicBool::new(false))?;
        println!("Saved proof {} (not sent to printer)", path.display());
    }
    Ok(())
}
