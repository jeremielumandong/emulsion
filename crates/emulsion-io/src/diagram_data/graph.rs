//! Local structural import for the other two Glyphtide engines. Neither parser
//! executes D2 imports, DOT URLs, scripts, or external rendering programs.
use super::syntax::{self, unquote};
use super::*;

fn shape(value: &str) -> ShapeKind {
    match value.trim_matches('"').to_ascii_lowercase().as_str() {
        "diamond" => ShapeKind::Decision,
        "circle" | "ellipse" | "oval" | "stadium" => ShapeKind::Terminator,
        "cylinder" | "database" => ShapeKind::Database,
        "parallelogram" => ShapeKind::Data,
        "note" | "page" => ShapeKind::Note,
        "class" => ShapeKind::Class,
        "sql_table" | "record" => ShapeKind::Entity,
        "cloud" => ShapeKind::Cloud,
        _ => ShapeKind::Process,
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    Symbol(char),
    Edge(String),
}

fn tokens(text: &str) -> Result<Vec<Token>> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '#' || (c == '/' && chars.peek() == Some(&'/')) {
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    closed = true;
                    break;
                }
            }
            if !closed {
                return Err(error("Unclosed DOT comment."));
            }
            continue;
        }
        if "{}[]=,:;".contains(c) {
            out.push(Token::Symbol(c));
            continue;
        }
        if c == '-' && chars.peek().is_some_and(|c| *c == '-' || *c == '>') {
            out.push(Token::Edge(format!("-{c}", c = chars.next().unwrap())));
            continue;
        }
        if c == '"' {
            let mut word = String::new();
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '"' {
                    closed = true;
                    break;
                }
                if c == '\\' {
                    let next = chars.next().ok_or_else(|| error("Unclosed DOT escape."))?;
                    match next {
                        'n' | 'l' | 'r' => word.push('\n'),
                        '\n' => {}
                        _ => word.push(next),
                    }
                } else {
                    word.push(c);
                }
            }
            if !closed {
                return Err(error("Unclosed DOT quoted string."));
            }
            out.push(Token::Word(word));
            continue;
        }
        if c == '<' {
            return Err(error(
                "DOT HTML labels are not supported; use quoted labels or import an SVG export.",
            ));
        }
        let mut word = c.to_string();
        while let Some(&next) = chars.peek() {
            if next.is_whitespace() || "{}[]=,:;\"<>".contains(next) || next == '-' {
                break;
            }
            word.push(chars.next().unwrap());
        }
        out.push(Token::Word(word));
        if out.len() > 100_000 {
            return Err(error("DOT token limit reached."));
        }
    }
    Ok(out)
}

struct Dot {
    tokens: Vec<Token>,
    at: usize,
    draft: Draft,
    directed: bool,
}
impl Dot {
    fn symbol(&mut self, c: char) -> bool {
        if self.tokens.get(self.at) == Some(&Token::Symbol(c)) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn word(&mut self) -> Result<String> {
        match self.tokens.get(self.at) {
            Some(Token::Word(w)) => {
                self.at += 1;
                Ok(w.clone())
            }
            _ => Err(error("Expected a DOT identifier or quoted value.")),
        }
    }
    fn attributes(&mut self) -> Result<BTreeMap<String, String>> {
        let mut attrs = BTreeMap::new();
        while self.symbol('[') {
            while !self.symbol(']') {
                if self.symbol(',') || self.symbol(';') {
                    continue;
                }
                let key = self.word()?;
                if !self.symbol('=') {
                    return Err(error("Expected '=' in DOT attributes."));
                }
                attrs.insert(key, self.word()?);
                if attrs.len() > 63 {
                    return Err(error("Too many DOT attributes."));
                }
            }
        }
        Ok(attrs)
    }
    fn port(&mut self) -> Result<()> {
        if self.symbol(':') {
            self.word()?;
            if self.symbol(':') {
                self.word()?;
            }
            self.draft
                .warn("DOT ports use automatic connector attachment.");
        }
        Ok(())
    }
    fn body(&mut self, depth: usize, mut defaults: BTreeMap<String, String>) -> Result<()> {
        if depth > 64 {
            return Err(error("DOT groups exceed 64 levels."));
        }
        while !self.symbol('}') {
            if self.symbol(';') {
                continue;
            }
            if self.symbol('{') {
                self.body(depth + 1, defaults.clone())?;
                continue;
            }
            let id = self.word()?;
            if id == "subgraph" {
                if !self.symbol('{') {
                    self.word()?;
                    if !self.symbol('{') {
                        return Err(error("Expected DOT subgraph body."));
                    }
                }
                self.draft
                    .warn("DOT subgraphs are flattened into editable shapes.");
                self.body(depth + 1, defaults.clone())?;
                continue;
            }
            if self.symbol('=') {
                let value = self.word()?;
                if id == "rankdir" && (value == "LR" || value == "RL") {
                    self.draft.layout = Layout::Horizontal;
                } else {
                    syntax::record(
                        &mut self.draft,
                        &format!("{id}={value}"),
                        "DOT graph attribute",
                    )?;
                }
                continue;
            }
            if ["node", "edge", "graph"].contains(&id.as_str())
                && self.tokens.get(self.at) == Some(&Token::Symbol('['))
            {
                let attrs = self.attributes()?;
                if id == "node" {
                    defaults.extend(attrs);
                } else {
                    if attrs.get("rankdir").is_some_and(|v| v == "LR" || v == "RL") {
                        self.draft.layout = Layout::Horizontal;
                    }
                    syntax::record(&mut self.draft, &format!("{id}: {attrs:?}"), "DOT defaults")?;
                    self.draft
                        .warn("DOT graph and edge styling is retained as notes.");
                }
                continue;
            }
            self.port()?;
            self.add_node(&id, &defaults)?;
            let mut previous = id.clone();
            let mut links = Vec::new();
            while let Some(Token::Edge(op)) = self.tokens.get(self.at) {
                let arrow = op == "->";
                if arrow != self.directed {
                    return Err(error("DOT edge operator does not match graph/digraph."));
                }
                self.at += 1;
                let next = self.word()?;
                self.port()?;
                self.add_node(&next, &defaults)?;
                links.push((previous, next.clone(), arrow));
                previous = next;
            }
            let attrs = self.attributes()?;
            if links.is_empty() {
                let item = self.draft.items.iter_mut().find(|i| i.key == id).unwrap();
                if let Some(label) = attrs.get("label") {
                    item.label = label.clone();
                }
                if let Some(kind) = attrs.get("shape") {
                    item.kind = shape(kind);
                }
                item.data
                    .extend(attrs.into_iter().map(|(k, v)| (format!("dot_{k}"), v)));
            } else {
                let label = attrs.get("label").map(String::as_str).unwrap_or("");
                for (a, b, arrow) in links {
                    if attrs.get("dir").is_some_and(|s| s == "back") {
                        self.draft.link(&b, &a, label, arrow)?;
                    } else {
                        self.draft.link(
                            &a,
                            &b,
                            label,
                            arrow && !attrs.get("dir").is_some_and(|s| s == "none"),
                        )?;
                        self.draft.links.last_mut().unwrap().arrow_start =
                            attrs.get("dir").is_some_and(|s| s == "both");
                    }
                }
                if attrs.keys().any(|k| k != "label" && k != "dir") {
                    syntax::record(
                        &mut self.draft,
                        &format!("{attrs:?}"),
                        "DOT edge attributes",
                    )?;
                }
            }
        }
        Ok(())
    }
    fn add_node(&mut self, id: &str, defaults: &BTreeMap<String, String>) -> Result<()> {
        if self.draft.items.iter().any(|i| i.key == id) {
            return Ok(());
        }
        let label = defaults.get("label").map(String::as_str).unwrap_or(id);
        self.draft.node(
            id,
            label,
            defaults
                .get("shape")
                .map_or(ShapeKind::Process, |s| shape(s)),
        )?;
        self.draft.items.last_mut().unwrap().data.extend(
            defaults
                .iter()
                .map(|(k, v)| (format!("dot_{k}"), v.clone())),
        );
        Ok(())
    }
}

pub(super) fn dot(text: &str) -> Result<Draft> {
    let mut parser = Dot {
        tokens: tokens(text.trim_start_matches('\u{feff}'))?,
        at: 0,
        draft: Draft::new(),
        directed: true,
    };
    let mut header = parser.word()?;
    if header == "strict" {
        header = parser.word()?;
    }
    parser.directed = match header.as_str() {
        "digraph" => true,
        "graph" => false,
        _ => return Err(error("Expected DOT graph or digraph.")),
    };
    if !parser.symbol('{') {
        parser.word()?;
        if !parser.symbol('{') {
            return Err(error("Expected a DOT graph body."));
        }
    }
    parser.body(0, BTreeMap::new())?;
    if parser.at != parser.tokens.len() {
        return Err(error("Unexpected content after the DOT graph."));
    }
    parser.draft.warn(
        "DOT imports nodes, labels, attributes and connections with Emulsion's layout and styling.",
    );
    Ok(parser.draft)
}

pub(super) fn d2(text: &str) -> Result<Draft> {
    let mut draft = Draft::new();
    let cleaned = text
        .lines()
        .map(|l| syntax::comment(l, "#"))
        .collect::<Vec<_>>()
        .join("\n");
    // Expand braces and semicolons outside strings, so compact object maps and
    // their multiline equivalents follow the same parser path.
    let positions = syntax::outside(&cleaned)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut expanded = String::new();
    for (i, c) in cleaned.char_indices() {
        if positions.contains(&i) && c == '{' {
            expanded.push_str("{\n");
        } else if positions.contains(&i) && c == '}' {
            expanded.push_str("\n}\n");
        } else if positions.contains(&i) && c == ';' {
            expanded.push('\n');
        } else {
            expanded.push(c);
        }
    }
    let mut scopes: Vec<String> = Vec::new();
    let mut edge_scopes: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for line in expanded.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if line == "}" {
            scopes
                .pop()
                .ok_or_else(|| error("Unexpected D2 block end."))?;
            continue;
        }
        if line.starts_with("...")
            || syntax::outside(line)?
                .into_iter()
                .any(|i| line[i..].starts_with('@') || line[i..].starts_with('|'))
        {
            return Err(error(
                "D2 imports, substitutions and block strings require an SVG export; they are not evaluated locally.",
            ));
        }
        let (statement, label) = syntax::pair(line, ':')?.unwrap_or((line, ""));
        let opens = label.trim_end().ends_with('{') || statement.trim_end().ends_with('{');
        let label = label.trim().trim_end_matches('{').trim();
        let statement = statement.trim().trim_end_matches('{').trim();
        if let Some(scope) = scopes.last()
            && let Some(edges) = edge_scopes.get(scope).cloned()
        {
            if statement == "label" {
                for i in &edges {
                    draft.links[*i].label = unquote(label);
                }
            }
            syntax::record(&mut draft, line, "D2 connection property")?;
            if opens {
                let scope = format!("{scope}.{statement}");
                edge_scopes.insert(scope.clone(), edges);
                scopes.push(scope);
            }
            continue;
        }
        if statement == "direction" {
            if label == "right" || label == "left" {
                draft.layout = Layout::Horizontal;
            }
            continue;
        }
        // Qualified property assignments describe an object, not a new node.
        // Keep field endpoints such as orders.customer_id as real IDs.
        let property = statement
            .find(".style")
            .map(|i| (&statement[..i], &statement[i + 1..]))
            .or_else(|| {
                statement.rsplit_once('.').filter(|(_, key)| {
                    [
                        "shape", "label", "class", "icon", "link", "tooltip", "width", "height",
                    ]
                    .contains(key)
                })
            });
        if let Some((owner, property)) = property {
            let key = scopes
                .last()
                .map_or_else(|| owner.to_string(), |parent| format!("{parent}.{owner}"));
            if !draft.items.iter().any(|i| i.key == key) {
                draft.node(&key, owner, ShapeKind::Process)?;
            }
            let item = draft.items.iter_mut().find(|i| i.key == key).unwrap();
            if property == "shape" {
                item.kind = shape(label);
            }
            if property == "label" {
                item.label = unquote(label);
            }
            item.data.insert(format!("d2_{property}"), unquote(label));
            if opens {
                scopes.push(format!("{key}.{property}"));
            }
            continue;
        }
        if [
            "shape",
            "label",
            "style",
            "icon",
            "link",
            "tooltip",
            "width",
            "height",
            "near",
            "constraint",
            "class",
            "grid-columns",
            "grid-rows",
        ]
        .contains(&statement)
            || statement.starts_with("style.")
            || scopes.last().is_some_and(|s| s.ends_with(".style"))
        {
            let parent = scopes
                .last()
                .map(String::as_str)
                .unwrap_or("")
                .trim_end_matches(".style");
            if let Some(item) = draft.items.iter_mut().find(|i| i.key == parent) {
                if statement == "shape" {
                    item.kind = shape(label);
                }
                if statement == "label" {
                    item.label = unquote(label);
                }
                item.data.insert(format!("d2_{statement}"), unquote(label));
            } else {
                syntax::record(&mut draft, line, "D2 property")?;
            }
            if opens {
                scopes.push(format!("{parent}.{statement}"));
            }
            continue;
        }
        let mut parts = Vec::new();
        let mut ops = Vec::new();
        let mut start = 0;
        for i in syntax::outside(statement)? {
            if i < start {
                continue;
            }
            if let Some(op) = ["<->", "->", "<-", "--"]
                .into_iter()
                .find(|op| statement[i..].starts_with(op))
            {
                parts.push(&statement[start..i]);
                ops.push(op);
                start = i + op.len();
            }
        }
        parts.push(&statement[start..]);
        let mut keys = Vec::new();
        for part in parts {
            let part = unquote(part);
            if part.is_empty() {
                return Err(error("Missing D2 connection endpoint."));
            }
            let key = scopes
                .last()
                .map_or_else(|| part.clone(), |parent| format!("{parent}.{part}"));
            if !draft.items.iter().any(|i| i.key == key) {
                draft.node(&key, &part, ShapeKind::Process)?;
            }
            keys.push(key);
        }
        let first_edge = draft.links.len();
        for (i, op) in ops.iter().enumerate() {
            if *op == "<-" {
                draft.link(&keys[i + 1], &keys[i], &unquote(label), true)?;
            } else {
                draft.link(&keys[i], &keys[i + 1], &unquote(label), *op != "--")?;
            }
            if *op == "<->" {
                draft.link(&keys[i + 1], &keys[i], &unquote(label), true)?;
            }
        }
        if ops.is_empty() {
            let item = draft.items.iter_mut().find(|i| i.key == keys[0]).unwrap();
            if !label.is_empty() {
                item.label = unquote(label);
            }
        }
        if opens {
            if !ops.is_empty() {
                let scope = format!("__edges_{first_edge}");
                edge_scopes.insert(scope.clone(), (first_edge..draft.links.len()).collect());
                scopes.push(scope);
                continue;
            }
            scopes.push(keys[0].clone());
            if scopes.len() > 64 {
                return Err(error("D2 groups exceed 64 levels."));
            }
        }
    }
    if !scopes.is_empty() {
        return Err(error("Unclosed D2 block."));
    }
    draft.warn("D2 imports editable nodes, labels, properties and connections. Containers are flattened and styling uses Emulsion defaults.");
    Ok(draft)
}
