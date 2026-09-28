//! Small bounded XML tree for OPC relationships and Visio's shape sheets.
use super::*;
use quick_xml::{Reader, events::Event};
#[derive(Clone, Debug, Default)]
pub(super) struct Xml {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    pub children: Vec<Xml>,
    pub text: String,
    /// Byte position in the parent’s mixed text, used by Visio cp/pp runs.
    pub text_offset: usize,
    pub image_part: Option<String>,
}
impl Xml {
    pub fn attr(&self, key: &str) -> &str {
        self.attrs.get(key).map(String::as_str).unwrap_or("")
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
                    text_offset: stack.last().unwrap().text.len(),
                    ..Default::default()
                };
                for a in e.attributes() {
                    let a = a.map_err(|e| error(e.to_string()))?;
                    let key = a.key.local_name().as_ref().to_string();
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
                stack.last_mut().unwrap().text.push_str(&node.text);
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
