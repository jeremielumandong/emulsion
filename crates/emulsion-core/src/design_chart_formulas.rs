//! Bounded local spreadsheet expressions. No scripts, IO, or external references.
use std::collections::HashSet;
type Cell = (usize, usize);
pub(super) fn resolve(rows: &[Vec<String>]) -> Result<Vec<Vec<String>>, String> {
    let mut engine = Engine {
        rows,
        cache: vec![vec![None; rows[0].len()]; rows.len()],
        visiting: HashSet::new(),
        budget: 100_000,
    };
    let mut resolved = rows.to_vec();
    for (r, row) in rows.iter().enumerate() {
        for (c, value) in row.iter().enumerate() {
            if value.trim_start().starts_with('=') {
                resolved[r][c] = engine.cell((r, c), false)?.unwrap_or(0.).to_string();
            }
        }
    }
    Ok(resolved)
}
struct Engine<'a> {
    rows: &'a [Vec<String>],
    cache: Vec<Vec<Option<f64>>>,
    visiting: HashSet<Cell>,
    budget: usize,
}
impl Engine<'_> {
    fn cell(&mut self, (r, c): Cell, skip_text: bool) -> Result<Option<f64>, String> {
        let source = self
            .rows
            .get(r)
            .and_then(|row| row.get(c))
            .ok_or_else(|| {
                format!(
                    "Cell {}{} is outside the table.",
                    (b'A' + c as u8) as char,
                    r + 1
                )
            })?
            .trim()
            .to_string();
        if let Some(value) = self.cache[r][c] {
            return Ok(Some(value));
        }
        if let Some(expr) = source.strip_prefix('=') {
            if self.visiting.len() >= 32 || !self.visiting.insert((r, c)) {
                return Err(format!(
                    "Circular or overly deep formula at {}{}.",
                    (b'A' + c as u8) as char,
                    r + 1
                ));
            }
            self.budget = self
                .budget
                .checked_sub(expr.len())
                .ok_or("Formula calculation exceeds its complexity limit.")?;
            let mut resolver = |cell, skip| self.cell(cell, skip);
            let mut parser = Parser {
                source: expr.as_bytes(),
                pos: 0,
                depth: 0,
                resolver: &mut resolver,
            };
            let value = parser
                .expr(0)
                .and_then(|value| {
                    parser.space();
                    if parser.pos != parser.source.len() {
                        Err("Unexpected formula input.".into())
                    } else {
                        finite(value)
                    }
                })
                .map_err(|e| format!("{}{}: {e}", (b'A' + c as u8) as char, r + 1))?;
            self.visiting.remove(&(r, c));
            self.cache[r][c] = Some(value);
            Ok(Some(value))
        } else if source.is_empty() {
            Ok(if skip_text { None } else { Some(0.) })
        } else {
            match source.parse::<f64>() {
                Ok(value) => finite(value).map(Some),
                Err(_) if skip_text => Ok(None),
                Err(_) => Err(format!(
                    "Cell {}{} is not numeric.",
                    (b'A' + c as u8) as char,
                    r + 1
                )),
            }
        }
    }
}
fn finite(value: f64) -> Result<f64, String> {
    if value.is_finite() && value.abs() <= 1e12 {
        Ok(value)
    } else {
        Err("Formula result must be finite within ±1e12.".into())
    }
}
fn address(value: &str) -> Result<Cell, String> {
    let value = value.replace('$', "").to_ascii_uppercase();
    let split = value.bytes().take_while(u8::is_ascii_alphabetic).count();
    if split != 1 || !matches!(value.as_bytes().first(), Some(b'A'..=b'I')) {
        return Err("Use cell addresses A1–I51.".into());
    }
    let row = value[split..]
        .parse::<usize>()
        .map_err(|_| "Use cell addresses A1–I51.")?;
    if !(1..=51).contains(&row) {
        return Err("Use cell addresses A1–I51.".into());
    }
    Ok((row - 1, (value.as_bytes()[0] - b'A') as usize))
}
struct Parser<'a, 'b> {
    source: &'a [u8],
    pos: usize,
    depth: usize,
    resolver: &'b mut dyn FnMut(Cell, bool) -> Result<Option<f64>, String>,
}
impl Parser<'_, '_> {
    fn space(&mut self) {
        while self
            .source
            .get(self.pos)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.pos += 1;
        }
    }
    fn eat(&mut self, ch: u8) -> bool {
        self.space();
        if self.source.get(self.pos) == Some(&ch) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn ident(&mut self) -> String {
        self.space();
        let start = self.pos;
        while self
            .source
            .get(self.pos)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'$' || *b == b'_')
        {
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.source[start..self.pos]).to_ascii_uppercase()
    }
    fn expr(&mut self, min: u8) -> Result<f64, String> {
        self.depth += 1;
        if self.depth > 32 {
            return Err("Formula nesting exceeds 32 levels.".into());
        }
        self.space();
        let mut value = if self.eat(b'-') {
            -self.expr(5)?
        } else if self.eat(b'+') {
            self.expr(5)?
        } else if self.eat(b'(') {
            let value = self.expr(0)?;
            if !self.eat(b')') {
                return Err("Missing closing parenthesis.".into());
            }
            value
        } else if self
            .source
            .get(self.pos)
            .is_some_and(|b| b.is_ascii_digit() || *b == b'.')
        {
            let start = self.pos;
            while self
                .source
                .get(self.pos)
                .is_some_and(|b| b.is_ascii_digit() || *b == b'.')
            {
                self.pos += 1;
            }
            if matches!(self.source.get(self.pos), Some(b'e' | b'E')) {
                self.pos += 1;
                if matches!(self.source.get(self.pos), Some(b'+' | b'-')) {
                    self.pos += 1;
                }
                while self.source.get(self.pos).is_some_and(u8::is_ascii_digit) {
                    self.pos += 1;
                }
            }
            String::from_utf8_lossy(&self.source[start..self.pos])
                .parse::<f64>()
                .map_err(|_| "Invalid formula number.")?
        } else {
            let name = self.ident();
            if name.is_empty() {
                return Err("Expected a number, cell reference or function.".into());
            }
            if self.eat(b'(') {
                self.function(&name)?
            } else {
                (self.resolver)(address(&name)?, false)?.unwrap_or(0.)
            }
        };
        loop {
            self.space();
            let Some(op) = self.source.get(self.pos).copied() else {
                break;
            };
            let (left, right) = match op {
                b'+' | b'-' => (1, 2),
                b'*' | b'/' => (3, 4),
                b'^' => (6, 5),
                _ => break,
            };
            if left < min {
                break;
            }
            self.pos += 1;
            let rhs = self.expr(right)?;
            value = finite(match op {
                b'+' => value + rhs,
                b'-' => value - rhs,
                b'*' => value * rhs,
                b'/' => {
                    if rhs == 0. {
                        return Err("Division by zero.".into());
                    }
                    value / rhs
                }
                _ => value.powf(rhs),
            })?;
        }
        self.depth -= 1;
        finite(value)
    }
    fn function(&mut self, name: &str) -> Result<f64, String> {
        if !matches!(
            name,
            "SUM" | "AVERAGE" | "MIN" | "MAX" | "COUNT" | "ABS" | "ROUND"
        ) {
            return Err(format!("Unsupported function {name}."));
        }
        let mut values = Vec::new();
        if !self.eat(b')') {
            loop {
                let start = self.pos;
                let first = self.ident();
                if !first.is_empty() && self.eat(b':') {
                    let a = address(&first)?;
                    let b = address(&self.ident())?;
                    if a.0 > b.0 || a.1 > b.1 {
                        return Err("Cell ranges must run from top-left to bottom-right.".into());
                    }
                    for r in a.0..=b.0 {
                        for c in a.1..=b.1 {
                            if let Some(value) = (self.resolver)((r, c), true)? {
                                values.push(value);
                            }
                        }
                    }
                } else {
                    self.pos = start;
                    values.push(self.expr(0)?);
                }
                if self.eat(b')') {
                    break;
                }
                if !self.eat(b',') {
                    return Err("Separate function arguments with commas.".into());
                }
            }
        }
        finite(match name {
            "SUM" => values.iter().sum(),
            "COUNT" => values.len() as f64,
            "AVERAGE" => {
                if values.is_empty() {
                    return Err("AVERAGE needs a numeric value.".into());
                }
                values.iter().sum::<f64>() / values.len() as f64
            }
            "MIN" => values.into_iter().reduce(f64::min).unwrap_or(0.),
            "MAX" => values.into_iter().reduce(f64::max).unwrap_or(0.),
            "ABS" => {
                if values.len() != 1 {
                    return Err("ABS needs one argument.".into());
                }
                values[0].abs()
            }
            "ROUND" => {
                if !(1..=2).contains(&values.len()) {
                    return Err("ROUND needs a value and optional decimal count.".into());
                }
                let digits = values.get(1).copied().unwrap_or(0.);
                if digits.fract() != 0. || !(-12. ..=12.).contains(&digits) {
                    return Err("ROUND decimals must be an integer from -12 to 12.".into());
                }
                let scale = 10f64.powf(digits);
                (values[0] * scale).round() / scale
            }
            _ => unreachable!(),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn grid(a: &str, b: &str) -> Vec<Vec<String>> {
        vec![
            vec!["Label".into(), "Value".into()],
            vec!["First".into(), a.into()],
            vec!["Second".into(), b.into()],
        ]
    }
    #[test]
    fn design_formulas_evaluate_references_ranges_precedence_and_keep_sources() {
        let rows = grid("10", "=ROUND(SUM(B2:B2)*2 + -2^2, 0)");
        assert_eq!(resolve(&rows).unwrap()[2][1], "16");
        assert!(rows[2][1].starts_with('='));
        assert_eq!(resolve(&grid("3", "=2^3^2")).unwrap()[2][1], "512");
        assert_eq!(
            resolve(&grid("=SUM(A1:B1)", "=COUNT(A1:B2)")).unwrap()[2][1],
            "1"
        );
        assert_eq!(
            resolve(&grid("1.5", "=AVERAGE(B2,2.5)")).unwrap()[2][1],
            "2"
        );
    }
    #[test]
    fn design_formulas_reject_cycles_external_refs_and_nonfinite_values() {
        for formula in [
            "=B3",
            "=1/0",
            "=J1",
            "=1e100",
            "=SUM(B3:B3)",
            "=fetch(1)",
            "=ABS(1,2)",
            "=SUM(B5:B2)",
            "=A1",
        ] {
            assert!(resolve(&grid("1", formula)).is_err(), "{formula}");
        }
        assert!(resolve(&grid("=B3", "=B2")).is_err());
    }
}
