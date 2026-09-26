//! Spike: does a render loop we own (winit + wgpu, Vello for vectors) beat
//! GPUI painting for Emulsion's workloads? See README.md and RESULTS.md.

mod app;
mod atlas;
mod bench;
mod brush;
mod cache;
mod canvas;
mod compositor;
mod engine;
mod fidelity;
mod gpu;
mod testdocs;
mod vector;

use anyhow::{Context, Result, bail};
use engine::{Engine, Offscreen, Output};
use gpu::{Gpu, TileFormat};
use std::path::PathBuf;
use std::time::Instant;
use vector::VectorSpace;

const USAGE: &str = "\
vello-canvas-spike <command>

  gen [--out DIR]                    write the test documents (default spikes/out)
  view FILE                          interactive window
  bench SCENARIO FILE                scripted run; SCENARIO is navigate, brush-a,
                                     brush-b or vector-edit
  fidelity FILE... [--out DIR] [--levels 0,1,2]
                                     pixel diff against the CPU compositor
  info                               adapter, formats and limits

options:
  --size WxH         window or offscreen size (default 1600x1000)
  --headless         render offscreen, no window (bench)
  --baseline         run the CPU baseline of the GPUI canvas instead (bench)
  --frames N         override the scenario's frame count
  --vsync            present with vsync (default off)
  --tiles unorm16|float16
  --vectors srgb|linear
  --no-vello         composite vector nodes from their CPU caches
  --no-cache         recomposite every layer every frame (no GPU tile cache)
  --json FILE        append the report as a JSON line
";

struct Args {
    positional: Vec<String>,
    size: (u32, u32),
    headless: bool,
    baseline: bool,
    frames: Option<usize>,
    vsync: bool,
    tiles: Option<TileFormat>,
    space: VectorSpace,
    vello: bool,
    cache: bool,
    json: Option<PathBuf>,
    out: Option<PathBuf>,
    levels: Vec<u32>,
}

fn parse() -> Result<Args> {
    let mut a = Args {
        positional: Vec::new(),
        size: (1600, 1000),
        headless: false,
        baseline: false,
        frames: None,
        vsync: false,
        tiles: None,
        space: VectorSpace::Srgb,
        vello: true,
        cache: true,
        json: None,
        out: None,
        levels: vec![0, 1, 2],
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().with_context(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').context("--size WxH")?;
                a.size = (w.parse()?, h.parse()?);
            }
            "--headless" => a.headless = true,
            "--baseline" => a.baseline = true,
            "--frames" => a.frames = Some(value()?.parse()?),
            "--vsync" => a.vsync = true,
            "--tiles" => {
                a.tiles = Some(TileFormat::parse(&value()?).context("--tiles unorm16|float16")?)
            }
            "--vectors" => {
                a.space = VectorSpace::parse(&value()?).context("--vectors srgb|linear")?
            }
            "--no-vello" => a.vello = false,
            "--no-cache" => a.cache = false,
            "--json" => a.json = Some(value()?.into()),
            "--out" => a.out = Some(value()?.into()),
            "--levels" => {
                a.levels = value()?
                    .split(',')
                    .map(|l| l.parse())
                    .collect::<Result<_, _>>()?
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            s if s.starts_with("--") => bail!("unknown option {s}\n\n{USAGE}"),
            _ => a.positional.push(arg),
        }
    }
    Ok(a)
}

fn init_tracing() {
    use tracing_subscriber::prelude::*;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            "info,wgpu_core=warn,wgpu_hal=warn,naga=warn,vello=warn,emulsion_io=warn",
        )
    });
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr));
    #[cfg(feature = "tracy")]
    let registry = registry.with(tracing_tracy::TracyLayer::default());
    registry.init();
}

fn open(path: &str) -> Result<emulsion_core::Document> {
    let t = Instant::now();
    let doc =
        emulsion_io::open(std::path::Path::new(path)).with_context(|| format!("open {path}"))?;
    tracing::info!(
        path,
        width = doc.width,
        height = doc.height,
        nodes = doc.nodes.len(),
        ms = t.elapsed().as_millis() as u64,
        "opened"
    );
    Ok(doc)
}

fn write_json(path: &Option<PathBuf>, value: serde_json::Value) -> Result<()> {
    if let Some(path) = path {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(f, "{value}")?;
    }
    Ok(())
}

fn commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn bench(args: &Args) -> Result<()> {
    let [_, scenario, file] = &args.positional[..] else {
        bail!("bench SCENARIO FILE\n\n{USAGE}");
    };
    let kind = bench::Kind::parse(scenario).context("unknown scenario")?;
    let mut doc = open(file)?;
    let paint = matches!(kind, bench::Kind::BrushA | bench::Kind::BrushB)
        .then(|| bench::add_paint_layer(&mut doc));
    let mut report = if args.baseline {
        bench::baseline(kind, &mut doc, paint, args.size)?
    } else if args.headless {
        let gpu = Gpu::new(gpu::instance(), None, args.tiles)?;
        let mut engine = Engine::new(
            gpu.clone(),
            &doc,
            paint,
            args.space,
            args.vello,
            args.cache,
            args.size,
        )?;
        let target = Offscreen::new(&gpu, args.size, wgpu::TextureFormat::Rgba8Unorm);
        let mut script = bench::Script::new(kind, &mut engine, args.frames);
        // Warm up pipelines and caches outside the measurement.
        engine.render(&target.view, target.format, Output::Encoded)?;
        gpu.wait();
        while !script.done() {
            let start = Instant::now();
            script.before_frame(&mut engine)?;
            let times = engine.render(&target.view, target.format, Output::Encoded)?;
            {
                let _span = tracing::info_span!("gpu_wait").entered();
                gpu.wait();
            }
            let done = Instant::now();
            script.after_frame(
                &mut engine,
                done.duration_since(start).as_secs_f64() * 1e3,
                done,
                &times,
            )?;
        }
        let mut report = std::mem::take(&mut script.report);
        report.mode = format!("headless, {}", gpu.describe());
        report.notes.push(format!(
            "GPU textures {:.0} MiB (atlas {} tiles in {} pages, {}); allocator {}",
            engine.texture_bytes() as f64 / 1048576.0,
            engine.atlas.used(),
            engine.atlas.pages(),
            gpu.tile_format.label(),
            gpu.allocated_bytes()
                .map_or("n/a".into(), |b| format!("{:.0} MiB", b as f64 / 1048576.0))
        ));
        report
    } else {
        let options = app::Options {
            size: args.size,
            vsync: args.vsync,
            tiles: args.tiles,
            space: args.space,
            vello: args.vello,
            cache: args.cache,
            script: Some((kind, args.frames)),
            title: format!("vello-canvas spike: {scenario} {file}"),
        };
        app::run(options, doc, paint)?.context("benchmark did not finish")?
    };
    report.notes.push(format!(
        "file {file}, {}x{} view, commit {}",
        args.size.0,
        args.size.1,
        commit()
    ));
    report.print();
    let mut json = report.json();
    json["file"] = file.clone().into();
    json["commit"] = commit().into();
    json["size"] = format!("{}x{}", args.size.0, args.size.1).into();
    json["options"] = serde_json::json!({
        "baseline": args.baseline,
        "headless": args.headless,
        "cache": args.cache,
        "vello": args.vello,
        "vsync": args.vsync,
        "tiles": args.tiles.map(|t| t.label()),
        "vectors": format!("{:?}", args.space),
    });
    write_json(&args.json, json)
}

fn main() -> Result<()> {
    init_tracing();
    let args = parse()?;
    match args.positional.first().map(String::as_str) {
        Some("gen") => testdocs::write_all(
            args.out
                .as_deref()
                .unwrap_or(std::path::Path::new("spikes/out")),
        ),
        Some("view") => {
            let file = args.positional.get(1).context("view FILE")?;
            let mut doc = open(file)?;
            let paint = bench::add_paint_layer(&mut doc);
            app::run(
                app::Options {
                    size: args.size,
                    vsync: args.vsync,
                    tiles: args.tiles,
                    space: args.space,
                    vello: args.vello,
                    cache: args.cache,
                    script: None,
                    title: format!("vello-canvas spike: {file}"),
                },
                doc,
                Some(paint),
            )?;
            Ok(())
        }
        Some("bench") => bench(&args),
        Some("fidelity") => {
            let gpu = Gpu::new(gpu::instance(), None, args.tiles)?;
            println!(
                "\nGPU: {}; tiles {}; commit {}\n\n{}",
                gpu.describe(),
                gpu.tile_format.label(),
                commit(),
                fidelity::HEADER
            );
            for file in &args.positional[1..] {
                let doc = open(file)?;
                let name = std::path::Path::new(file)
                    .file_stem()
                    .map_or("doc".into(), |s| s.to_string_lossy().into_owned());
                let diffs = fidelity::run(&gpu, &doc, &name, args.out.as_deref(), &args.levels)?;
                for d in diffs {
                    write_json(
                        &args.json,
                        serde_json::json!({
                            "fidelity": d.label, "pixels": d.pixels,
                            "max_linear": d.max_linear, "mean_linear": d.mean_linear,
                            "max_code": d.max_code, "over_1": d.over_1, "over_3": d.over_3,
                            "edges": d.edges, "interior_over_3": d.interior_over_3,
                            "tiles": gpu.tile_format.label(), "commit": commit(),
                        }),
                    )?;
                }
            }
            Ok(())
        }
        Some("info") => {
            let gpu = Gpu::new(gpu::instance(), None, args.tiles)?;
            println!("{}", gpu.describe());
            println!("tile format: {}", gpu.tile_format.label());
            let l = gpu.adapter.limits();
            println!(
                "max 2D {}, array layers {}, storage buffer {} MiB",
                l.max_texture_dimension_2d,
                l.max_texture_array_layers,
                l.max_storage_buffer_binding_size >> 20
            );
            for f in [
                wgpu::TextureFormat::Rgba16Unorm,
                wgpu::TextureFormat::Rgba16Float,
            ] {
                println!("{f:?}: {:?}", gpu.adapter.get_texture_format_features(f));
            }
            Ok(())
        }
        _ => {
            print!("{USAGE}");
            Ok(())
        }
    }
}
