//! Placing a shape's label inside or just outside the shape.
use super::*;

/// Vertical placement: a row inside the shape, or just above or below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelRow {
    Above,
    Top,
    Middle,
    Bottom,
    Below,
}
impl LabelRow {
    pub const ALL: [Self; 5] = [
        Self::Above,
        Self::Top,
        Self::Middle,
        Self::Bottom,
        Self::Below,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Above => "above",
            Self::Top => "top",
            Self::Middle => "middle",
            Self::Bottom => "bottom",
            Self::Below => "below",
        }
    }
    pub fn outside(self) -> bool {
        matches!(self, Self::Above | Self::Below)
    }
}

/// Horizontal placement within the label frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelColumn {
    Left,
    Center,
    Right,
}
impl LabelColumn {
    pub const ALL: [Self; 3] = [Self::Left, Self::Center, Self::Right];
    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
}

const INSET: f64 = 8.;
const GAP: f64 = 5.;

fn text_height(spec: &TextSpec) -> f64 {
    let b = crate::text::layout(spec).bounds();
    f64::from(b.y + b.height).max(f64::from(spec.size * spec.line_height))
}

fn shape_label(doc: &Document, id: NodeId) -> Result<(Bounds, NodeId, &TextSpec), String> {
    let shape = doc
        .diagram
        .as_ref()
        .and_then(|d| d.shapes.get(&id))
        .ok_or("Select a shape to position its label.")?;
    let bounds = shape_bounds(doc, shape).ok_or("The shape has no geometry.")?;
    match &doc
        .node(shape.label)
        .ok_or("The shape label is missing.")?
        .kind
    {
        NodeKind::Text { spec, .. } => Ok((bounds, shape.label, spec)),
        _ => Err("The shape label is not text.".into()),
    }
}

/// The label frame for a position, as one undoable text edit.
pub fn label_position_command(
    doc: &Document,
    shape: NodeId,
    row: LabelRow,
    column: LabelColumn,
) -> Result<Command, String> {
    let ([x, y, w, h], label, spec) = shape_label(doc, shape)?;
    let mut spec = spec.clone();
    let inset = if row.outside() { 0. } else { INSET };
    spec.x = (x + inset) as f32;
    spec.width = Some((w - 2. * inset).max(1.) as f32);
    // Outside labels must not be clipped to the shape.
    spec.height = None;
    spec.align = match column {
        LabelColumn::Left => Align::Left,
        LabelColumn::Center => Align::Center,
        LabelColumn::Right => Align::Right,
    };
    let text = text_height(&spec);
    spec.y = match row {
        LabelRow::Above => y - GAP - text,
        LabelRow::Top => y + INSET,
        LabelRow::Middle => y + (h - text) / 2.,
        LabelRow::Bottom => y + h - INSET - text,
        LabelRow::Below => y + h + GAP,
    } as f32;
    Ok(Command::SetText {
        id: label,
        spec: Box::new(spec),
    })
}

/// The position that best describes the label's current frame.
pub fn label_position(doc: &Document, shape: NodeId) -> Option<(LabelRow, LabelColumn)> {
    let ([_, y, _, h], _, spec) = shape_label(doc, shape).ok()?;
    let column = match spec.align {
        Align::Left => LabelColumn::Left,
        Align::Right => LabelColumn::Right,
        Align::Center | Align::Justify => LabelColumn::Center,
    };
    let (top, text) = (f64::from(spec.y), text_height(spec));
    let row = if top >= y + h - 1. {
        LabelRow::Below
    } else if top + text <= y + 1. {
        LabelRow::Above
    } else {
        [
            (LabelRow::Top, y + INSET),
            (LabelRow::Middle, y + (h - text) / 2.),
            (LabelRow::Bottom, y + h - INSET - text),
        ]
        .into_iter()
        .min_by(|a, b| (a.1 - top).abs().total_cmp(&(b.1 - top).abs()))?
        .0
    };
    Some((row, column))
}

/// Fill empty labels that sit below their artwork, as vendor icons do, with
/// the entry name so placed icons are captioned like the source library.
pub fn caption_icon_labels(doc: &mut Document, name: &str) {
    let Some(model) = doc.diagram.clone() else {
        return;
    };
    let (w, h) = (doc.width, doc.height);
    for (id, shape) in &model.shapes {
        if label_position(doc, *id).is_none_or(|(row, _)| row != LabelRow::Below) {
            continue;
        }
        if let Some(Node {
            kind: NodeKind::Text { spec, cache },
            ..
        }) = doc.node_mut(shape.label)
            && spec.text.trim().is_empty()
        {
            Arc::make_mut(spec).text = name.to_string();
            *cache = crate::vector_cache::VectorRaster::text(spec.clone(), w, h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape() -> (Editor, NodeId) {
        let mut editor = Editor::new(Document::new(400, 300), None);
        let id = add_shape(
            &mut editor,
            ShapeKind::Process,
            [100., 100., 120., 80.],
            "Label",
        )
        .unwrap();
        (editor, id)
    }

    #[test]
    fn every_position_round_trips_and_undoes() {
        let (mut editor, id) = shape();
        let before = editor.doc.clone();
        for row in LabelRow::ALL {
            for column in LabelColumn::ALL {
                let command = label_position_command(&editor.doc, id, row, column).unwrap();
                editor.execute(command).unwrap();
                assert_eq!(label_position(&editor.doc, id), Some((row, column)));
            }
        }
        for _ in 0..LabelRow::ALL.len() * LabelColumn::ALL.len() {
            assert!(editor.undo());
        }
        assert_eq!(editor.doc, before);
    }

    #[test]
    fn outside_labels_clear_the_shape() {
        let (mut editor, id) = shape();
        let label = editor.doc.diagram.as_ref().unwrap().shapes[&id].label;
        for (row, check) in [
            (
                LabelRow::Below,
                (|y: f32, _h: f64| y >= 180.) as fn(f32, f64) -> bool,
            ),
            (LabelRow::Above, |y, h| f64::from(y) + h <= 100.),
        ] {
            let command =
                label_position_command(&editor.doc, id, row, LabelColumn::Center).unwrap();
            editor.execute(command).unwrap();
            let NodeKind::Text { spec, .. } = &editor.doc.node(label).unwrap().kind else {
                unreachable!()
            };
            assert!(check(spec.y, text_height(spec)), "{row:?}");
            assert_eq!(spec.width, Some(120.));
        }
    }

    #[test]
    fn empty_icon_captions_take_the_entry_name() {
        let (mut editor, id) = shape();
        let label = editor.doc.diagram.as_ref().unwrap().shapes[&id].label;
        let mut command =
            label_position_command(&editor.doc, id, LabelRow::Below, LabelColumn::Center).unwrap();
        if let Command::SetText { spec, .. } = &mut command {
            spec.text.clear();
        }
        editor.execute(command).unwrap();
        let mut doc = editor.doc.clone();
        caption_icon_labels(&mut doc, "Amazon EC2");
        assert!(
            matches!(&doc.node(label).unwrap().kind, NodeKind::Text { spec, .. } if spec.text == "Amazon EC2")
        );
        // Inside labels are left alone.
        let (editor, id) = shape();
        let mut doc = editor.doc.clone();
        caption_icon_labels(&mut doc, "Ignored");
        let label = doc.diagram.as_ref().unwrap().shapes[&id].label;
        assert!(
            matches!(&doc.node(label).unwrap().kind, NodeKind::Text { spec, .. } if spec.text == "Label")
        );
    }
}
