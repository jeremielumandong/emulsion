//! Embedded data images only. Import never fetches external URLs.
use super::*;
use emulsion_raster::{Placement, Raster};

pub(crate) fn insert(
    doc: &mut Document,
    parent: NodeId,
    mut bounds: [f64; 4],
    preserve_aspect: bool,
    uri: &str,
    pixels_remaining: &mut usize,
    warnings: &mut BTreeSet<String>,
) -> Result<()> {
    let Some(data) = uri.strip_prefix("data:") else {
        warnings.insert(
            "External image URLs are not embedded; replace the image from a local file.".into(),
        );
        return Ok(());
    };
    let (mime, payload) = data
        .split_once(',')
        .ok_or_else(|| error("Invalid embedded image URI"))?;
    let svg_text = if mime.starts_with("image/svg+xml") && !mime.ends_with(";base64") {
        Some(percent_decode(payload)?)
    } else {
        None
    };
    let bytes = if mime.ends_with(";base64")
        || !mime.starts_with("image/svg+xml")
        || svg_text
            .as_ref()
            .is_some_and(|s| !s.trim_start().starts_with('<'))
    {
        base64::engine::general_purpose::STANDARD
            .decode(payload)
            .map_err(|e| error(format!("Invalid image data: {e}")))?
    } else {
        svg_text.unwrap_or_default().into_bytes()
    };
    if bytes.len() > MAX_BYTES {
        return Err(error("Embedded image exceeds 32 MiB"));
    }
    let mut svg_source = None;
    let raster = if mime.starts_with("image/svg+xml") {
        let text = std::str::from_utf8(&bytes).map_err(|e| error(e.to_string()))?;
        if preserve_aspect {
            let tree=resvg::usvg::Tree::from_str(text,&crate::svg_vectors::options()).map_err(|e|error(e.to_string()))?;
            bounds=fit(bounds,f64::from(tree.size().width()),f64::from(tree.size().height()));
        }
        match crate::svg_vectors::append(doc, parent, text, bounds) {
            Ok(notes) => {
                warnings.extend(notes);
                return Ok(());
            }
            Err(reason) => {
                warnings.insert(format!(
                    "SVG retained as scalable artwork rather than separate editable paths: {reason}"
                ));
                svg_source = Some(Arc::<str>::from(text));
            }
        }
        use resvg::{tiny_skia, usvg};
        static FONTS: std::sync::OnceLock<Arc<usvg::fontdb::Database>> = std::sync::OnceLock::new();
        let fonts = FONTS.get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        });
        let options = usvg::Options {
            fontdb: fonts.clone(),
            image_href_resolver: usvg::ImageHrefResolver {
                resolve_string: Box::new(|_, _| None),
                ..Default::default()
            },
            ..Default::default()
        };
        let tree = usvg::Tree::from_str(text, &options).map_err(|e| error(e.to_string()))?;
        let w = bounds[2].ceil().clamp(1., 1024.) as u32;
        let h = bounds[3].ceil().clamp(1., 1024.) as u32;
        let mut pixmap =
            tiny_skia::Pixmap::new(w, h).ok_or_else(|| error("Cannot allocate embedded image"))?;
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(
                w as f32 / tree.size().width(),
                h as f32 / tree.size().height(),
            ),
            &mut pixmap.as_mut(),
        );
        let rgba: Vec<u8> = pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect();
        Raster::from_srgba8(w, h, &rgba)
    } else {
        let mut reader =
            image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(64 << 20);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|e| error(e.to_string()))?
            .to_rgba8();
        Raster::from_srgba8(image.width(), image.height(), image.as_raw())
    };
    let pixels = raster.width() as usize * raster.height() as usize;
    *pixels_remaining = pixels_remaining
        .checked_sub(pixels)
        .ok_or_else(|| error("Embedded images exceed the 16 megapixel page limit"))?;
    if preserve_aspect && svg_source.is_none(){bounds=fit(bounds,f64::from(raster.width()),f64::from(raster.height()));}
    let [x, y, w, h] = bounds;
    let placement = Placement {
        x,
        y,
        scale_x: w / raster.width() as f64,
        scale_y: h / raster.height() as f64,
        ..Default::default()
    };
    let id = doc.alloc_id();
    let mut node = Node::raster(id, "Embedded draw.io image", Arc::new(raster), placement);
    if let Some(xml) = svg_source
        && let NodeKind::Raster { raster, placement } = &node.kind
    {
        node.kind = NodeKind::Smart {
            editable: Some(emulsion_core::node::SmartEditable::Svg { xml }),
            source: raster.clone(),
            filters: Vec::new(),
            filter_styles: Vec::new(),
            placement: *placement,
            cache: raster.clone(),
            offset: (0, 0),
        };
        node.name = "Scalable SVG artwork".into();
    }
    node.parent = Some(parent);
    let label = doc
        .nodes
        .iter()
        .position(|n| n.parent == Some(parent) && matches!(n.kind, NodeKind::Text { .. }))
        .unwrap_or(doc.nodes.len());
    doc.nodes.insert(label, node);
    warnings.insert(
        "Embedded artwork is retained; save the native project to preserve its source.".into(),
    );
    Ok(())
}

fn fit([x,y,w,h]:[f64;4],sw:f64,sh:f64)->[f64;4] {
    let scale=(w/sw).min(h/sh);let width=sw*scale;let height=sh*scale;
    [x+(w-width)/2.,y+(h-height)/2.,width,height]
}
