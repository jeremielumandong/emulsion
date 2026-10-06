//! Bounded local graph drafts from text, CSV, Mermaid, D2, DOT and SQL schemas.
//! Parsers never evaluate expressions, execute SQL, or fetch remote resources.
use crate::{IoError, Result};
use emulsion_core::{
    Command, Document, Editor, NodeKind,
    diagram::{self, Endpoint, Layout, Port, Routing, ShapeKind},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
const MAX_BYTES: usize = 1 << 20;
mod graph;
mod mermaid;
#[cfg(test)]
mod source_tests;
mod syntax;
fn error(value: impl Into<String>) -> IoError {
    IoError::Manifest(value.into())
}
#[derive(Clone, Copy, Debug)]
pub enum Format {
    Text,
    Csv,
    Mermaid,
    D2,
    Graphviz,
    Sql,
}
impl Format {
    pub const ALL: [Self; 6] = [
        Self::Text,
        Self::Csv,
        Self::Mermaid,
        Self::D2,
        Self::Graphviz,
        Self::Sql,
    ];
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "txt" => Some(Self::Text),
            "csv" => Some(Self::Csv),
            "mmd" | "mermaid" => Some(Self::Mermaid),
            "d2" => Some(Self::D2),
            "dot" | "gv" => Some(Self::Graphviz),
            "sql" => Some(Self::Sql),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text flow",
            Self::Csv => "CSV",
            Self::Mermaid => "Mermaid",
            Self::D2 => "D2",
            Self::Graphviz => "Graphviz DOT",
            Self::Sql => "SQL schema",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Item {
    pub key: String,
    pub label: String,
    pub kind: ShapeKind,
    pub data: BTreeMap<String, String>,
}
#[derive(Clone, Debug)]
pub struct Link {
    pub source: String,
    pub target: String,
    pub label: String,
    pub arrow: bool,
    pub arrow_start: bool,
}
#[derive(Clone, Debug)]
pub struct Draft {
    pub items: Vec<Item>,
    pub links: Vec<Link>,
    pub layout: Layout,
    pub warnings: Vec<String>,
    mermaid_source: Option<String>,
}
impl Draft {
    pub fn is_mermaid(&self) -> bool {
        self.mermaid_source.is_some()
    }
    fn new() -> Self {
        Self {
            items: Vec::new(),
            links: Vec::new(),
            layout: Layout::Vertical,
            warnings: Vec::new(),
            mermaid_source: None,
        }
    }
    fn warn(&mut self, warning: &str) {
        if !self.warnings.iter().any(|s| s == warning) {
            self.warnings.push(warning.into());
        }
    }
    fn node(&mut self, key: &str, label: &str, kind: ShapeKind) -> Result<()> {
        if key.trim().is_empty() || key.len() > 200 || label.chars().count() > 2000 {
            return Err(error(
                "Shape IDs need 1–200 bytes and labels up to 2,000 characters.",
            ));
        }
        if let Some(item) = self.items.iter_mut().find(|i| i.key == key) {
            if label != key {
                item.label = label.into();
                item.kind = kind;
            }
            return Ok(());
        }
        if self.items.len() >= diagram::MAX_SHAPES {
            return Err(error("Too many generated shapes."));
        }
        self.items.push(Item {
            key: key.into(),
            label: label.into(),
            kind,
            data: BTreeMap::new(),
        });
        Ok(())
    }
    fn link(&mut self, a: &str, b: &str, label: &str, arrow: bool) -> Result<()> {
        if self.links.len() >= diagram::MAX_EDGES || label.chars().count() > 2000 {
            return Err(error(
                "Too many connections or an oversized connection label.",
            ));
        }
        self.links.push(Link {
            source: a.into(),
            target: b.into(),
            label: label.into(),
            arrow,
            arrow_start: false,
        });
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        if self.items.is_empty()
            || self.items.len() > diagram::MAX_SHAPES
            || self.links.len() > diagram::MAX_EDGES
        {
            return Err(error(
                "A draft needs 1–1,000 shapes and at most 2,000 connections.",
            ));
        }
        let ids = self.items.iter().map(|i| &i.key).collect::<BTreeSet<_>>();
        if ids.len() != self.items.len() {
            return Err(error("Duplicate shape ID."));
        }
        for item in &self.items {
            if item.key.trim().is_empty()
                || item.key.len() > 200
                || item.key.chars().any(char::is_control)
                || item.label.chars().count() > 2000
                || item.data.len() > 63
                || item
                    .data
                    .iter()
                    .any(|(k, v)| k.len() > 128 || v.len() > 4096 || k == "source_id")
            {
                return Err(error(
                    "A shape ID, label or data field exceeds the supported limits, or uses the reserved source_id field.",
                ));
            }
        }
        for edge in &self.links {
            if !ids.contains(&edge.source) || !ids.contains(&edge.target) {
                return Err(error(format!(
                    "Connection refers to a missing shape: {} → {}",
                    edge.source, edge.target
                )));
            }
        }
        Ok(())
    }
    /// Materialize into a new page; callers install it only after this succeeds.
    pub fn document(&self) -> Result<Document> {
        self.validate()?;
        if let Some(source) = &self.mermaid_source {
            return mermaid::document(source);
        }
        let columns = (self.items.len() as f64).sqrt().ceil().max(2.) as u32;
        let rows = (self.items.len() as u32).div_ceil(columns);
        let mut builder =
            diagram::Builder::new((columns * 280 + 80).max(1000), (rows * 220 + 80).max(800))
                .map_err(error)?;
        let mut ids = BTreeMap::new();
        for (index, item) in self.items.iter().enumerate() {
            let height = if matches!(
                item.kind,
                ShapeKind::Entity | ShapeKind::Class | ShapeKind::Note
            ) {
                (item.label.lines().count() as f64 * 22. + 32.).clamp(100., 800.)
            } else {
                80.
            };
            let id = builder
                .add_shape(
                    item.kind,
                    [
                        40. + (index as u32 % columns) as f64 * 280.,
                        40. + (index as u32 / columns) as f64 * 220.,
                        200.,
                        height,
                    ],
                    &item.label,
                )
                .map_err(error)?;
            ids.insert(item.key.clone(), id);
        }
        for edge in &self.links {
            builder
                .connect(
                    Endpoint {
                        shape: ids[&edge.source],
                        port: Port::Auto,
                    },
                    Endpoint {
                        shape: ids[&edge.target],
                        port: Port::Auto,
                    },
                    &edge.label,
                    Routing::Orthogonal,
                )
                .map_err(error)?;
        }
        let mut editor = Editor::try_new(builder.finish().map_err(error)?, None)?;
        let mut model = editor.doc.diagram.as_deref().unwrap().clone();
        for item in &self.items {
            let shape = model.shapes.get_mut(&ids[&item.key]).unwrap();
            shape.data = item.data.clone();
            shape.data.insert("source_id".into(), item.key.clone());
        }
        for (edge, source) in model.edges.values_mut().zip(&self.links) {
            edge.arrow_end = source.arrow;
            edge.arrow_start = source.arrow_start;
        }
        editor
            .execute(Command::SetDiagram {
                diagram: Some(Arc::new(model)),
            })
            .map_err(|e| error(e.to_string()))?;
        diagram::arrange(&mut editor, self.layout).map_err(error)?;
        let bounds = editor
            .doc
            .diagram
            .as_ref()
            .unwrap()
            .shapes
            .values()
            .filter_map(|s| diagram::shape_bounds(&editor.doc, s))
            .collect::<Vec<_>>();
        let width = bounds
            .iter()
            .map(|b| b[0] + b[2] + 40.)
            .fold(editor.doc.width as f64, f64::max)
            .ceil() as u32;
        let height = bounds
            .iter()
            .map(|b| b[1] + b[3] + 40.)
            .fold(editor.doc.height as f64, f64::max)
            .ceil() as u32;
        if width > 30000 || height > 30000 {
            return Err(error(
                "The generated layout exceeds 30,000 pixels. Split the input into smaller diagrams.",
            ));
        }
        editor
            .execute(Command::Crop {
                rect: emulsion_raster::IRect::new(0, 0, width as i32, height as i32),
                rotation: 0.,
            })
            .map_err(|e| error(e.to_string()))?;
        Ok(editor.doc)
    }
    /// Refresh mapped labels and data without moving shapes or replacing links.
    pub fn refresh_commands(&self, doc: &Document) -> Result<Vec<Command>> {
        if self.is_mermaid() {
            return Err(error(
                "Re-import Mermaid source to update its rendered layout. Data refresh applies to mapped native graph shapes.",
            ));
        }
        self.validate()?;
        let mut graph = doc
            .diagram
            .as_deref()
            .cloned()
            .ok_or_else(|| error("This page has no mapped diagram shapes."))?;
        let mut mapped = BTreeMap::new();
        for (id, shape) in &graph.shapes {
            if let Some(key) = shape.data.get("source_id")
                && mapped.insert(key.clone(), *id).is_some()
            {
                return Err(error(
                    "Several shapes have the same source_id. Assign unique IDs before refreshing.",
                ));
            }
        }
        let mut commands = Vec::new();
        for item in &self.items {
            let id = *mapped.get(&item.key).ok_or_else(|| {
                error(format!(
                    "No existing shape maps to {}. Import as a new page for structural changes.",
                    item.key
                ))
            })?;
            let shape = graph.shapes.get_mut(&id).unwrap();
            shape.data = item.data.clone();
            shape.data.insert("source_id".into(), item.key.clone());
            let Some(NodeKind::Text { spec, .. }) = doc.node(shape.label).map(|n| &n.kind) else {
                return Err(error("Mapped shape has no editable label."));
            };
            let mut spec = (**spec).clone();
            spec.text = item.label.clone();
            spec.runs.clear();
            commands.push(Command::SetText {
                id: shape.label,
                spec: Box::new(spec),
            });
        }
        commands.push(Command::SetDiagram {
            diagram: Some(Arc::new(graph)),
        });
        let mut trial = doc.clone();
        for command in &commands {
            command
                .apply(&mut trial)
                .map_err(|e| error(e.to_string()))?;
        }
        Ok(commands)
    }
}
pub fn parse(text: &str, format: Format) -> Result<Draft> {
    if text.len() > MAX_BYTES {
        return Err(error("Diagram input exceeds 1 MiB."));
    }
    let draft = match format {
        Format::Text => plain(text),
        Format::Csv => csv(text),
        Format::Mermaid => mermaid::parse(text),
        Format::D2 => graph::d2(text),
        Format::Graphviz => graph::dot(text),
        Format::Sql => sql(text),
    }?;
    draft.validate()?;
    Ok(draft)
}
fn plain(text: &str) -> Result<Draft> {
    let mut draft = Draft::new();
    let mut previous: Option<String> = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let parts = line.split("->").map(str::trim).collect::<Vec<_>>();
        for part in &parts {
            draft.node(part, part, ShapeKind::Process)?;
        }
        if parts.len() == 1 {
            if let Some(before) = previous.take() {
                draft.link(&before, parts[0], "", true)?;
            }
            previous = Some(parts[0].to_string());
        } else {
            previous = None;
            for pair in parts.windows(2) {
                draft.link(pair[0], pair[1], "", true)?;
            }
        }
    }
    Ok(draft)
}
/// Quoted fields, embedded newlines, doubled quotes and CRLF are supported.
pub(crate) fn csv_rows(text: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !closed => quoted = true,
            ',' => {
                row.push(std::mem::take(&mut field));
                closed = false;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                closed = false;
            }
            _ if closed => return Err(error("Unexpected characters after a quoted CSV field.")),
            '"' => return Err(error("Quote inside an unquoted CSV field.")),
            _ => field.push(c),
        }
        if rows.len() > diagram::MAX_SHAPES + 1 || row.len() > 64 || field.len() > 4096 {
            return Err(error("CSV exceeds its row, column or field limits."));
        }
    }
    if quoted {
        return Err(error("Unterminated quoted CSV field."));
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}
fn csv(text: &str) -> Result<Draft> {
    let rows = csv_rows(text)?;
    let header = rows
        .first()
        .ok_or_else(|| error("CSV needs a header row."))?
        .iter()
        .map(|s| s.trim().to_lowercase())
        .collect::<Vec<_>>();
    if header.iter().any(String::is_empty)
        || header.iter().collect::<BTreeSet<_>>().len() != header.len()
    {
        return Err(error("CSV column names must be nonempty and unique."));
    }
    let key=header.iter().position(|s|s=="id").ok_or_else(||error("CSV requires an id column; optional label, type, next and edge_label columns describe shapes and connections."))?;
    let mut draft = Draft::new();
    let mut seen = BTreeSet::new();
    for (index, row) in rows.iter().enumerate().skip(1) {
        if row.len() != header.len() {
            return Err(error(format!(
                "CSV row {} has the wrong number of columns.",
                index + 1
            )));
        }
        let id = row[key].trim();
        if !seen.insert(id) {
            return Err(error(format!("Duplicate CSV id: {id}")));
        }
        let get = |name: &str| {
            header
                .iter()
                .position(|h| h == name)
                .map(|i| row[i].trim())
                .unwrap_or("")
        };
        let kind = match get("type").to_lowercase().as_str() {
            "" | "process" => ShapeKind::Process,
            "decision" => ShapeKind::Decision,
            "start" | "end" | "terminator" => ShapeKind::Terminator,
            "database" => ShapeKind::Database,
            "entity" => ShapeKind::Entity,
            "note" => ShapeKind::Note,
            other => return Err(error(format!("Unsupported CSV shape type: {other}"))),
        };
        let label = get("label");
        draft.node(id, if label.is_empty() { id } else { label }, kind)?;
        let item = draft.items.last_mut().unwrap();
        for (column, value) in header.iter().zip(row) {
            if !matches!(
                column.as_str(),
                "id" | "label" | "type" | "next" | "edge_label"
            ) {
                item.data.insert(column.clone(), value.clone());
            }
        }
        for target in get("next")
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            draft.link(id, target, get("edge_label"), true)?;
        }
    }
    Ok(draft)
}
// SQL is a schema-only tokenizer. DML, functions and arbitrary scripts are rejected.
fn sql_tokens(text: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '-' && chars.peek() == Some(&'-') {
            chars.next();
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            continue;
        }
        if c.is_whitespace() || "(),;".contains(c) {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            if !c.is_whitespace() {
                out.push(c.to_string());
            }
        } else if matches!(c, '"' | '`' | '[' | '\'') {
            if !word.is_empty() {
                return Err(error("Invalid quoted SQL identifier."));
            }
            let close = if c == '[' { ']' } else { c };
            let mut closed = false;
            while let Some(ch) = chars.next() {
                if ch == close {
                    if chars.peek() == Some(&close) {
                        chars.next();
                        word.push(ch);
                    } else {
                        closed = true;
                        break;
                    }
                } else {
                    word.push(ch);
                }
            }
            if !closed {
                return Err(error("Unclosed SQL quoted identifier or literal."));
            }
            out.push(std::mem::take(&mut word));
        } else {
            word.push(c);
        }
        if out.len() > 100_000 {
            return Err(error("SQL schema token limit reached."));
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    Ok(out)
}
fn sql(text: &str) -> Result<Draft> {
    let tokens = sql_tokens(text)?;
    let mut i = 0;
    let mut draft = Draft::new();
    let mut tables = BTreeSet::new();
    let mut references = Vec::new();
    while i < tokens.len() {
        if tokens[i] == ";" {
            i += 1;
            continue;
        }
        if !tokens[i].eq_ignore_ascii_case("CREATE")
            || !tokens
                .get(i + 1)
                .is_some_and(|s| s.eq_ignore_ascii_case("TABLE"))
        {
            return Err(error("SQL import accepts CREATE TABLE statements only."));
        }
        i += 2;
        if tokens.get(i).is_some_and(|s| s.eq_ignore_ascii_case("IF")) {
            if !tokens
                .get(i + 1)
                .is_some_and(|s| s.eq_ignore_ascii_case("NOT"))
                || !tokens
                    .get(i + 2)
                    .is_some_and(|s| s.eq_ignore_ascii_case("EXISTS"))
            {
                return Err(error("Expected IF NOT EXISTS."));
            }
            i += 3;
        }
        let table = tokens
            .get(i)
            .ok_or_else(|| error("Missing SQL table name."))?
            .clone();
        i += 1;
        if !tables.insert(table.clone()) {
            return Err(error("Duplicate SQL table name."));
        }
        if tokens.get(i).map(String::as_str) != Some("(") {
            return Err(error("Expected a SQL table column list."));
        }
        i += 1;
        let mut depth = 1usize;
        let mut column = Vec::new();
        let mut columns = Vec::new();
        while i < tokens.len() && depth > 0 {
            let token = &tokens[i];
            i += 1;
            if token == "(" {
                depth += 1;
            } else if token == ")" {
                depth -= 1;
            }
            if depth == 0 || (depth == 1 && token == ",") {
                if !column.is_empty() {
                    columns.push(std::mem::take(&mut column));
                }
            } else {
                column.push(token.clone());
            }
        }
        if depth != 0 || columns.is_empty() {
            return Err(error("Unclosed or empty SQL table."));
        }
        let mut label = table.clone();
        let mut data = BTreeMap::new();
        for col in columns {
            let constraint = col[0].eq_ignore_ascii_case("CONSTRAINT")
                || col[0].eq_ignore_ascii_case("PRIMARY")
                || col[0].eq_ignore_ascii_case("FOREIGN")
                || col[0].eq_ignore_ascii_case("UNIQUE")
                || col[0].eq_ignore_ascii_case("CHECK");
            if !constraint {
                if col.len() < 2 {
                    return Err(error("A SQL column needs a name and type."));
                }
                label.push_str(&format!("\n{}: {}", col[0], col[1]));
                data.insert(col[0].clone(), col[1..].join(" "));
            }
            if let Some(r) = col
                .iter()
                .position(|t| t.eq_ignore_ascii_case("REFERENCES"))
            {
                let target = col
                    .get(r + 1)
                    .ok_or_else(|| error("Missing referenced table."))?;
                references.push((
                    table.clone(),
                    target.clone(),
                    if constraint {
                        "foreign key".into()
                    } else {
                        col[0].clone()
                    },
                ));
            }
        }
        draft.node(&table, &label, ShapeKind::Entity)?;
        draft.items.last_mut().unwrap().data = data;
    }
    for (a, b, label) in references {
        draft.link(&a, &b, &label, true)?;
    }
    Ok(draft)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_supports_quoted_fields_branches_and_local_metadata() {
        let draft=parse("id,label,type,next,edge_label,owner\r\na,\"Review, then approve\",decision,b;c,Yes,Pat\r\nb,Publish,process,,,Lee\r\nc,\"Wait\nfor changes\",note,,,Sam\r\n",Format::Csv).unwrap();
        assert_eq!(draft.items.len(), 3);
        assert_eq!(draft.links.len(), 2);
        assert_eq!(draft.items[0].data["owner"], "Pat");
        assert_eq!(draft.items[2].label, "Wait\nfor changes");
        let doc = draft.document().unwrap();
        let graph = doc.diagram.as_ref().unwrap();
        assert_eq!(graph.edges.len(), 2);
        assert!(
            graph
                .shapes
                .values()
                .any(|s| s.data.get("source_id").is_some_and(|v| v == "a"))
        );
        assert!(
            doc.nodes
                .iter()
                .all(|n| !matches!(n.kind, NodeKind::Raster { .. }))
        );
    }
    #[test]
    fn mermaid_uses_its_renderer_instead_of_generic_graph_layout() {
        let draft = parse(
            "flowchart LR\nA[Start] --> B{Ready?}\nB -->|Yes| C((Done))\nB -->|No| A",
            Format::Mermaid,
        )
        .unwrap();
        assert!(draft.is_mermaid());
        assert!(draft.links.is_empty());
        let svg = mermaid::svg(draft.mermaid_source.as_deref().unwrap()).unwrap();
        assert!(svg.contains("Start") && svg.contains("Ready?") && svg.contains("Yes"));
        draft.document().unwrap().validate().unwrap();
    }
    #[test]
    fn sql_schema_creates_editable_entities_and_foreign_keys() {
        let draft=parse("CREATE TABLE users (id INTEGER PRIMARY KEY, name VARCHAR(100));\nCREATE TABLE orders (id INT, user_id INT REFERENCES users(id), amount DECIMAL(10,2), CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id));",Format::Sql).unwrap();
        assert_eq!(draft.items.len(), 2);
        assert_eq!(draft.links.len(), 2);
        assert!(draft.items[1].label.contains("user_id: INT"));
        assert_eq!(draft.items[1].kind, ShapeKind::Entity);
        draft.document().unwrap().validate().unwrap();
    }
    #[test]
    fn refresh_keeps_ids_positions_and_connections_and_rejects_unknown_mappings() {
        let draft = parse(
            "id,label,next,owner\na,Start,b,Pat\nb,Review,,Lee",
            Format::Csv,
        )
        .unwrap();
        let mut editor = Editor::new(draft.document().unwrap(), None);
        let before = editor.doc.clone();
        let update = parse("id,label,owner\na,New label,Sam", Format::Csv).unwrap();
        let commands = update.refresh_commands(&editor.doc).unwrap();
        editor.begin("Refresh data");
        for command in commands {
            editor.execute(command).unwrap();
        }
        editor.end();
        let model = editor.doc.diagram.as_ref().unwrap();
        assert_eq!(model.edges, before.diagram.as_ref().unwrap().edges);
        for (id, shape) in &model.shapes {
            assert_eq!(
                diagram::shape_bounds(&editor.doc, shape),
                diagram::shape_bounds(&before, &before.diagram.as_ref().unwrap().shapes[id])
            );
        }
        assert!(
            model
                .shapes
                .values()
                .any(|s| s.data.get("owner").is_some_and(|v| v == "Sam"))
        );
        assert!(
            parse("id,label\nmissing,Unknown", Format::Csv)
                .unwrap()
                .refresh_commands(&editor.doc)
                .is_err()
        );
        editor.undo();
        assert_eq!(editor.doc, before);
    }
    #[test]
    fn malformed_and_unsupported_input_is_rejected_before_installation() {
        for (input, format) in [
            ("id,label,next\na,Start,missing", Format::Csv),
            ("id,label\na,First\na,Second", Format::Csv),
            ("id,label\na,\"unfinished", Format::Csv),
            ("flowchart TD\nsubgraph hidden\nA --> B", Format::Mermaid),
            ("flowchart TD\nA -->|bad B", Format::Mermaid),
            (
                "CREATE TABLE a (id INT REFERENCES missing(id));",
                Format::Sql,
            ),
            ("DROP TABLE a;", Format::Sql),
            ("CREATE TABLE a (id INT", Format::Sql),
        ] {
            assert!(parse(input, format).is_err(), "accepted {input}");
        }
        assert!(parse(&"x".repeat(MAX_BYTES + 1), Format::Text).is_err());
    }
}
