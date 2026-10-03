//! Export reproducible, editable Design fixtures through the actual project writers.
//! cargo run -p emulsion-io --example design_export_fidelity -- NEW_OUTPUT_DIRECTORY
//! Optionally append an existing project path. Input projects are never modified.
use emulsion_core::{
    Command, Document, Editor, Node, NodeKind,
    command::Slot,
    design::{self, Element, Template},
    project::{Project, ProjectEditor, ProjectKind},
    styles::LayerStyle,
    text::TextSpec,
};
use emulsion_raster::{Placement, Raster, composite::flatten, vector::PathStyle};
use std::{error::Error, io::Read, path::Path, sync::Arc};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn add(doc: &mut Document, node: Node) -> Result<u64> {
    Ok(Command::AddNode {
        node: Box::new(node),
        slot: Slot::TOP,
    }
    .apply(doc)?
    .unwrap())
}
fn glow(color: [u8; 3], size: f32) -> LayerStyle {
    LayerStyle::DropShadow {
        color,
        opacity: 100.,
        angle: 90.,
        distance: 0.,
        size,
    }
}
fn styled_fixture(outer: bool) -> Result<Document> {
    let (w, h) = (900, 600);
    let mut d = Document::new(w, h);
    d.resolution = 300.;
    add(
        &mut d,
        Node::new(
            0,
            "Opaque dark background",
            NodeKind::Fill {
                rgba: [13, 20, 36, 255],
            },
        ),
    )?;
    let mut t = Node::text(
        0,
        "Glowing editable text",
        TextSpec {
            text: "NEON / EXPORT".into(),
            font: "Geist".into(),
            size: 72.,
            bold: true,
            color: [240, 252, 255, 255],
            x: 76.,
            y: 100.,
            ..Default::default()
        },
        w,
        h,
    );
    t.styles = vec![if outer {
        LayerStyle::OuterGlow {
            color: [39, 227, 248],
            opacity: 100.,
            size: 35.,
        }
    } else {
        glow([39, 227, 248], 35.)
    }];
    add(&mut d, t)?;
    let mut n = Node::path(
        0,
        "Glowing vector circle",
        Arc::new(Element::Circle.path(120., 280., 185., 185.)),
        PathStyle {
            fill: Some([254, 173, 214, 255]),
            stroke: None,
            ..Default::default()
        },
        w,
        h,
    );
    n.styles = vec![glow([255, 50, 142], 48.)];
    add(&mut d, n)?;
    let mut n = Node::path(
        0,
        "Offset shadow rectangle",
        Arc::new(Element::Rectangle.path(460., 285., 275., 155.)),
        PathStyle {
            fill: Some([153, 229, 255, 255]),
            stroke: None,
            ..Default::default()
        },
        w,
        h,
    );
    n.styles = vec![LayerStyle::DropShadow {
        color: [75, 95, 255],
        opacity: 100.,
        angle: 45.,
        distance: 42.,
        size: 24.,
    }];
    add(&mut d, n)?;
    add(
        &mut d,
        Node::text(
            0,
            "Footer",
            TextSpec {
                text: "Editable text + shape effects / 300 PPI".into(),
                font: "Geist".into(),
                size: 23.,
                color: [177, 192, 221, 255],
                x: 76.,
                y: 528.,
                ..Default::default()
            },
            w,
            h,
        ),
    )?;
    Ok(d)
}
fn photo_fixture() -> Result<Document> {
    let (w, h) = (900, 600);
    let mut d = Document::new(w, h);
    d.resolution = 144.;
    let background = Arc::new(Raster::from_fn(w, h, [0; 4], |x, y| {
        let a = (x * 35000 / w) as u16;
        let b = (y * 22000 / h) as u16;
        [2500 + a / 4, 7000 + b / 2, 18000 + a / 2, 65535]
    }));
    add(
        &mut d,
        Node::raster(
            0,
            "Photo-like gradient background",
            background,
            Placement::default(),
        ),
    )?;
    let mut e = Editor::new(d, None);
    let g = design::frame(&e.doc, Element::Circle)
        .paste(&mut e, Slot::TOP, (0., 0.))
        .map_err(std::io::Error::other)?[0];
    let photo = Arc::new(Raster::from_fn(480, 320, [0; 4], |x, y| {
        let stripe = if (x / 40 + y / 40) % 2 == 0 { 7000 } else { 0 };
        [
            (10000 + x * 90).min(65535) as u16,
            (9000 + y * 135).min(65535) as u16,
            18000 + stripe,
            65535,
        ]
    }));
    design::place_in_frame(&mut e, g, photo).map_err(std::io::Error::other)?;
    e.doc.node_mut(g).unwrap().styles = vec![LayerStyle::DropShadow {
        color: [0, 0, 0],
        opacity: 80.,
        angle: 60.,
        distance: 16.,
        size: 30.,
    }];
    add(
        &mut e.doc,
        Node::text(
            0,
            "Photo title",
            TextSpec {
                text: "PHOTO / FRAME".into(),
                font: "Geist".into(),
                size: 48.,
                bold: true,
                color: [255, 255, 255, 255],
                x: 52.,
                y: 45.,
                ..Default::default()
            },
            w,
            h,
        ),
    )?;
    Ok(e.doc)
}
fn fixtures() -> Result<Project> {
    let mut p = ProjectEditor::new_project(ProjectKind::Design, styled_fixture(false)?)
        .map_err(std::io::Error::other)?;
    p.rename_page(1, "Text and shape glow".into(), 0.)
        .map_err(std::io::Error::other)?;
    p.add_page(photo_fixture()?, "Photo frame and background".into(), 0.)
        .map_err(std::io::Error::other)?;
    let t = Template::VideoThumbnail;
    let (w, h) = t.native_size();
    p.add_page(
        t.create(w, h).map_err(std::io::Error::other)?,
        "Native video thumbnail".into(),
        0.,
    )
    .map_err(std::io::Error::other)?;
    for t in Template::catalog().filter(|t| t.label().to_ascii_lowercase().contains("omarchy")) {
        let (w, h) = t.native_size();
        p.add_page(
            t.create(w, h).map_err(std::io::Error::other)?,
            t.label().into(),
            0.,
        )
        .map_err(std::io::Error::other)?;
    }
    p.add_page(
        styled_fixture(true)?,
        "Outer glow compositor fallback".into(),
        0.,
    )
    .map_err(std::io::Error::other)?;
    p.snapshot()
        .ok_or_else(|| "Missing project snapshot".into())
}
fn write_png(path: &Path, w: u32, h: u32, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, emulsion_io::export::png8(w, h, bytes)?)?;
    Ok(())
}
fn verify_png_samples(project: &Project, selected: &[u64], path: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let pages: Vec<_> = project
        .pages
        .iter()
        .filter(|page| selected.contains(&page.meta.id))
        .collect();
    assert_eq!(archive.len(), pages.len());
    for (index, page) in pages.into_iter().enumerate() {
        let mut bytes = Vec::new();
        archive.by_index(index)?.read_to_end(&mut bytes)?;
        let png = image::load_from_memory(&bytes)?.into_rgba16();
        assert_eq!(png.dimensions(), (page.doc.width, page.doc.height));
        assert_eq!(
            png.into_raw(),
            flatten(&page.doc.composite_tree(), 0).to_srgba16(),
            "Actual PNG export changes native samples on {}",
            page.meta.name
        );
    }
    Ok(())
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let out = std::path::PathBuf::from(args.next().ok_or("Pass an output directory")?);
    std::fs::create_dir_all(&out)?;
    if std::fs::read_dir(&out)?.next().is_some() {
        return Err("Choose a new or empty output directory".into());
    }
    let input = args.next().map(std::path::PathBuf::from);
    if args.next().is_some() {
        return Err("Unexpected argument".into());
    }
    let before = input.as_ref().map(std::fs::read).transpose()?;
    let p = match &input {
        Some(path) => emulsion_io::project::read(path)?,
        None => fixtures()?,
    };
    p.validate().map_err(std::io::Error::other)?;
    let source = out.join("editable-source.emu");
    emulsion_io::project::write(&p, &source)?;
    let saved = std::fs::read(&source)?;
    let p = emulsion_io::project::read(&source)?;
    let pristine = p.clone();
    let mut manifest =
        String::from("page\tid\tname\twidth\theight\tppi\tstyles\tstrict_svg\tviewport\n");
    for (index, page) in p.pages.iter().enumerate() {
        let d = &page.doc;
        let stem = format!("page-{:02}", index + 1);
        let native = flatten(&d.composite_tree(), 0).to_srgba8();
        write_png(
            &out.join(format!("{stem}-native.png")),
            d.width,
            d.height,
            &native,
        )?;
        let strict = match emulsion_io::project_export::vector_svg(d) {
            Ok(svg) => {
                std::fs::write(out.join(format!("{stem}-strict.svg")), &svg)?;
                let tree = resvg::usvg::Tree::from_data(&svg, &Default::default())?;
                let mut pix = resvg::tiny_skia::Pixmap::new(d.width, d.height).unwrap();
                resvg::render(
                    &tree,
                    resvg::tiny_skia::Transform::identity(),
                    &mut pix.as_mut(),
                );
                pix.save_png(out.join(format!("{stem}-svg.png")))?;
                true
            }
            Err(err) => {
                println!("{stem} strict SVG unavailable: {err}");
                false
            }
        };
        let viewport = match emulsion_io::svg_viewport::SvgViewport::new(d) {
            Ok(v) => {
                let mut bytes = v.render((d.width, d.height), [1., 0., 0., 1., 0., 0.])?;
                for p in bytes.as_chunks_mut::<4>().0 {
                    p.swap(0, 2);
                    let a = u32::from(p[3]);
                    if a > 0 && a < 255 {
                        for c in &mut p[..3] {
                            *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
                        }
                    }
                }
                write_png(
                    &out.join(format!("{stem}-viewport.png")),
                    d.width,
                    d.height,
                    &bytes,
                )?;
                true
            }
            Err(err) => {
                println!("{stem} viewport falls back to compositor: {err}");
                false
            }
        };
        let styles = d.nodes.iter().map(|n| n.styles.len()).sum::<usize>();
        manifest.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            index + 1,
            page.meta.id,
            page.meta.name,
            d.width,
            d.height,
            d.resolution,
            styles,
            strict,
            viewport
        ));
        println!(
            "{stem}: {} {}x{} @ {} PPI, {styles} effects",
            page.meta.name, d.width, d.height, d.resolution
        );
    }
    let ids: Vec<_> = p.pages.iter().map(|page| page.meta.id).collect();
    let selected = if ids.len() > 1 {
        vec![ids[(ids.len() - 1).min(2)], ids[0]]
    } else {
        vec![ids[0]]
    };
    for (name, selection) in [("all-pages", ids), ("selected-pages", selected)] {
        for (format, ext) in [
            (emulsion_io::project_export::Format::Png, "zip"),
            (emulsion_io::project_export::Format::Pdf, "pdf"),
        ] {
            let report = emulsion_io::project_export::write(
                &p,
                &selection,
                format,
                false,
                &out.join(format!("{name}.{ext}")),
            )?;
            assert_eq!(report.pages, selection.len());
            if format == emulsion_io::project_export::Format::Png {
                verify_png_samples(&p, &selection, &out.join(format!("{name}.{ext}")))?;
            }
            println!("{name}.{ext}: {report:?}");
        }
    }
    assert_eq!(saved, std::fs::read(source)?);
    if let (Some(input), Some(before)) = (input, before) {
        assert_eq!(before, std::fs::read(input)?);
    }
    for (a, b) in p.pages.iter().zip(&pristine.pages) {
        assert_eq!(a.doc, b.doc);
        assert_eq!(a.meta, b.meta);
        assert_eq!(a.graph.head(), b.graph.head());
        assert_eq!(a.graph.len(), b.graph.len());
    }
    std::fs::write(out.join("manifest.tsv"), manifest)?;
    println!("Source .emu and editable document state preserved.");
    Ok(())
}
