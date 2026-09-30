//! Quote-aware scanning shared by the local diagram importers.
use super::*;

pub(super) fn unquote(value: &str) -> String {
    let value = value.trim();
    let value = if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    };
    value
        .replace("\\n", "\n")
        .replace("\\\"", "\"")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n")
}

/// Positions outside quoted strings and node delimiters. Byte offsets always
/// come from char_indices, including for non-ASCII labels.
pub(super) fn outside(text: &str) -> Result<Vec<usize>> {
    let mut positions = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut stack = Vec::new();
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if c == '"' {
            quote = Some(c);
            continue;
        }
        if stack.is_empty() {
            positions.push(i);
        }
        match c {
            '[' => stack.push(']'),
            '(' => stack.push(')'),
            ']' | ')' if stack.last() == Some(&c) => {
                stack.pop();
            }
            _ => {}
        }
    }
    if quote.is_some() || !stack.is_empty() {
        return Err(error("Unclosed diagram label or quoted string."));
    }
    Ok(positions)
}

pub(super) fn split(text: &str, delimiter: char) -> Result<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    for i in outside(text)? {
        if text[i..].starts_with(delimiter) {
            parts.push(&text[start..i]);
            start = i + delimiter.len_utf8();
        }
    }
    parts.push(&text[start..]);
    Ok(parts)
}

pub(super) fn pair(text: &str, delimiter: char) -> Result<Option<(&str, &str)>> {
    Ok(outside(text)?
        .into_iter()
        .find(|&i| text[i..].starts_with(delimiter))
        .map(|i| (&text[..i], &text[i + delimiter.len_utf8()..])))
}

pub(super) fn comment<'a>(text: &'a str, marker: &str) -> &'a str {
    let mut quote = false;
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && quote {
            escaped = true;
            continue;
        }
        if c == '"' {
            quote = !quote;
        }
        if !quote && text[i..].starts_with(marker) {
            return &text[..i];
        }
    }
    text
}

pub(super) fn record(draft: &mut Draft, line: &str, category: &str) -> Result<String> {
    let key = format!("__record_{}", draft.items.len());
    draft.node(&key, line.trim(), ShapeKind::Note)?;
    let item = draft.items.last_mut().unwrap();
    item.data.insert("diagram_type".into(), category.into());
    item.data.insert("source".into(), line.into());
    Ok(key)
}
