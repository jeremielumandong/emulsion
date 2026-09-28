//! Small bounded XML tree for OPC relationships and Visio's shape sheets.
use super::*;
use quick_xml::{Reader, events::Event};
use std::io::Read;
#[derive(Clone, Debug, Default)]
pub(super) struct Xml {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    pub children: Vec<Xml>,
    pub text: String,
}
impl Xml {
    pub fn attr(&self, key: &str) -> &str {
        self.attrs
            .get(key)
            .or_else(|| {
                key.strip_prefix("r:").and_then(|local| {
                    self.attrs
                        .iter()
                        .find(|(name, _)| {
                            name.contains(':') && name.rsplit(':').next() == Some(local)
                        })
                        .map(|(_, value)| value)
                })
            })
            .map(String::as_str)
            .unwrap_or("")
    }
    pub fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|n| n.name == name)
    }
    pub fn children(&self, name: &str) -> impl Iterator<Item = &Self> {
        self.children.iter().filter(move |n| n.name == name)
    }
    pub fn descendants<'a>(&'a self, name: &'a str) -> Box<dyn Iterator<Item = &'a Self> + 'a> {
        Box::new(self.children.iter().flat_map(move |n| {
            std::iter::once(n)
                .filter(move |n| n.name == name)
                .chain(n.descendants(name))
        }))
    }
}
pub(super) fn parse(text: &str) -> Result<Xml> {
    if !valid_xml_text(text) {
        return Err(error("Invalid XML control character"));
    }
    if text.len() > 16 << 20 {
        return Err(error("XML part exceeds 16 MiB."));
    }
    let mut reader = Reader::from_str(text);
    let mut stack = vec![Xml::default()];
    let mut count = 0;
    loop {
        let event = reader.read_event().map_err(|e| error(e.to_string()))?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                count += 1;
                if count > 100_000 || stack.len() > 64 {
                    return Err(error("XML element or depth limit exceeded."));
                }
                let mut node = Xml {
                    name: e.local_name().as_ref().to_string(),
                    ..Default::default()
                };
                for a in e.attributes() {
                    let a = a.map_err(|e| error(e.to_string()))?;
                    let key = a.key.as_ref().to_string();
                    let value = a
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|e| error(e.to_string()))?
                        .to_string();
                    if node.attrs.insert(key, value).is_some() {
                        return Err(error("Duplicate XML attribute."));
                    }
                }
                if empty {
                    stack.last_mut().unwrap().children.push(node);
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    return Err(error("Unexpected XML close."));
                }
                let node = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(node);
            }
            Event::Text(t) => {
                let t = t.xml10_content();
                stack
                    .last_mut()
                    .unwrap()
                    .text
                    .push_str(&quick_xml::escape::unescape(&t).map_err(|e| error(e.to_string()))?);
            }
            Event::CData(t) => stack.last_mut().unwrap().text.push_str(t.as_ref()),
            Event::DocType(_) => {
                return Err(error("Document type declarations are not supported."));
            }
            Event::GeneralRef(r) => {
                let name = r.as_ref();
                let entity = format!("&{name};");
                stack.last_mut().unwrap().text.push_str(
                    &quick_xml::escape::unescape(&entity).map_err(|e| error(e.to_string()))?,
                );
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 || stack[0].children.len() != 1 {
        return Err(error("Malformed XML document."));
    }
    Ok(stack.pop().unwrap().children.remove(0))
}

pub(super) struct Package {
    parts: BTreeMap<String, Vec<u8>>,
}
#[derive(Clone, Debug)]
pub(super) struct Rel {
    pub kind: String,
    pub target: String,
    pub external: bool,
}
impl Package {
    pub fn read(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        if file.metadata()?.len() > 128 << 20 {
            return Err(error("PPTX exceeds 128 MiB compressed limit"));
        }
        let mut zip = zip::ZipArchive::new(file)?;
        if zip.len() > 4096 {
            return Err(error("PPTX exceeds 4096 package parts"));
        }
        let mut parts = BTreeMap::new();
        let mut expanded = 0u64;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i)?;
            if f.is_dir() {
                continue;
            }
            let name = f.name().to_string();
            if name.starts_with('/')
                || name.contains('\\')
                || name.split('/').any(|p| p == ".." || p == ".")
            {
                return Err(error("Invalid PPTX part path"));
            }
            expanded = expanded
                .checked_add(f.size())
                .ok_or_else(|| error("Package size overflow"))?;
            if expanded > 256 << 20 || f.size() > 64 << 20 {
                return Err(error("PPTX exceeds expanded package limits"));
            }
            let mut data = Vec::new();
            f.by_ref().take((64 << 20) + 1).read_to_end(&mut data)?;
            if data.len() > 64 << 20 || parts.insert(name, data).is_some() {
                return Err(error("Oversized or duplicate PPTX part"));
            }
        }
        Ok(Self { parts })
    }
    pub fn bytes(&self, name: &str) -> Result<&[u8]> {
        self.parts
            .get(name)
            .map(Vec::as_slice)
            .ok_or_else(|| error(format!("Missing PPTX part {name}")))
    }
    pub fn has(&self, name: &str) -> bool {
        self.parts.contains_key(name)
    }
    pub fn xml(&self, name: &str) -> Result<Xml> {
        let data = self.bytes(name)?;
        if data.len() > 16 << 20 {
            return Err(error("PPTX XML part exceeds 16 MiB"));
        }
        parse(std::str::from_utf8(data).map_err(|_| error("PPTX XML must be UTF-8"))?)
    }
    pub fn rels(&self, part: &str) -> Result<BTreeMap<String, Rel>> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let path = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        if !self.has(&path) {
            return Ok(BTreeMap::new());
        }
        let xml = self.xml(&path)?;
        let mut out = BTreeMap::new();
        for r in xml.children("Relationship") {
            let external = r.attr("TargetMode") == "External";
            let target = if external {
                r.attr("Target").to_string()
            } else {
                resolve(part, r.attr("Target"))?
            };
            if out
                .insert(
                    r.attr("Id").to_string(),
                    Rel {
                        kind: r.attr("Type").into(),
                        target,
                        external,
                    },
                )
                .is_some()
            {
                return Err(error("Duplicate relationship identifier"));
            }
        }
        Ok(out)
    }
}
pub(super) fn resolve(part: &str, target: &str) -> Result<String> {
    if target.contains(['\\', '\0', '?', '#', ':']) {
        return Err(error("Unsafe internal PPTX relationship"));
    }
    let mut path = if target.starts_with('/') {
        Vec::new()
    } else {
        part.rsplit_once('/').map_or(Vec::new(), |(d, _)| {
            d.split('/').map(str::to_string).collect()
        })
    };
    for p in target.trim_start_matches('/').split('/') {
        match p {
            "." | "" => (),
            ".." => {
                if path.pop().is_none() {
                    return Err(error("PPTX relationship escapes package"));
                }
            }
            _ => path.push(p.into()),
        }
    }
    Ok(path.join("/"))
}
