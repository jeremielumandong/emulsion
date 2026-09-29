//! Bounded numeric ShapeSheet subset; never executes code or external references.
use super::{Xml, value};
pub(super) fn cell(node: &Xml, master: Option<&Xml>, key: &str, depth: usize) -> Option<f64> {
    cell_budget(node, master, key, depth, &mut 512)
}
fn cell_budget(
    node: &Xml,
    master: Option<&Xml>,
    key: &str,
    depth: usize,
    budget: &mut usize,
) -> Option<f64> {
    if depth > 24 || *budget == 0 {
        return None;
    }
    *budget -= 1;
    if let Some(v) = value(node, key)
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| v.is_finite())
    {
        return Some(v);
    }
    let formula = node
        .children
        .iter()
        .find(|c| c.name == "Cell" && c.attr("N") == key)
        .map(|c| c.attr("F"))
        .filter(|s| !s.is_empty() && *s != "Inh");
    if let Some(f) = formula {
        let mut p = Parser {
            rest: f.trim_start_matches('='),
            node,
            master,
            depth,
            budget,
        };
        let v = p.expr(0)?;
        if p.rest.trim().is_empty() && v.is_finite() && v.abs() <= 1e6 {
            Some(v)
        } else {
            None
        }
    } else {
        master.and_then(|m| cell_budget(m, None, key, depth + 1, budget))
    }
}
struct Parser<'a, 'b> {
    rest: &'a str,
    node: &'a Xml,
    master: Option<&'a Xml>,
    depth: usize,
    budget: &'b mut usize,
}
impl Parser<'_, '_> {
    fn eat(&mut self, c: char) -> bool {
        self.rest = self.rest.trim_start();
        if let Some(r) = self.rest.strip_prefix(c) {
            self.rest = r;
            true
        } else {
            false
        }
    }
    fn expr(&mut self, level: usize) -> Option<f64> {
        let a = self.sum(level)?;
        self.rest = self.rest.trim_start();
        for operator in ["<=", ">=", "<>", "=", "<", ">"] {
            if let Some(rest) = self.rest.strip_prefix(operator) {
                self.rest = rest;
                let b = self.sum(level)?;
                if !a.is_finite() || !b.is_finite() {
                    return None;
                }
                return Some(f64::from(match operator {
                    "<=" => a <= b,
                    ">=" => a >= b,
                    "<>" => a != b,
                    "=" => a == b,
                    "<" => a < b,
                    _ => a > b,
                }));
            }
        }
        Some(a)
    }
    fn sum(&mut self, level: usize) -> Option<f64> {
        if level > 32 || *self.budget == 0 {
            return None;
        }
        *self.budget -= 1;
        let mut v = self.term(level + 1)?;
        loop {
            if self.eat('+') {
                v += self.term(level + 1)?;
            } else if self.eat('-') {
                v -= self.term(level + 1)?;
            } else {
                break;
            }
        }
        Some(v)
    }
    fn term(&mut self, level: usize) -> Option<f64> {
        let mut v = self.atom(level + 1)?;
        loop {
            if self.eat('*') {
                v *= self.atom(level + 1)?;
            } else if self.eat('/') {
                v /= self.atom(level + 1)?;
            } else {
                break;
            }
        }
        Some(v)
    }
    fn atom(&mut self, level: usize) -> Option<f64> {
        if level > 32 || *self.budget == 0 {
            return None;
        }
        *self.budget -= 1;
        if self.eat('-') {
            return Some(-self.atom(level + 1)?);
        }
        if self.eat('+') {
            return self.atom(level + 1);
        }
        if self.eat('(') {
            let v = self.expr(level + 1)?;
            return self.eat(')').then_some(v);
        }
        self.rest = self.rest.trim_start();
        let len = self
            .rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        if len > 0 && self.rest.as_bytes()[0].is_ascii_alphabetic() {
            let name = &self.rest[..len];
            self.rest = &self.rest[len..];
            if self.eat('(') {
                if name.eq_ignore_ascii_case("PI") {
                    return self.eat(')').then_some(std::f64::consts::PI);
                }
                let v = self.expr(level + 1)?;
                let v = match name.to_ascii_uppercase().as_str() {
                    "GUARD" | "THEMEGUARD" => v,
                    "ABS" => v.abs(),
                    "SQRT" => v.sqrt(),
                    "SIN" => v.sin(),
                    "COS" => v.cos(),
                    "TAN" => v.tan(),
                    "ASIN" => v.asin(),
                    "ACOS" => v.acos(),
                    "ATAN" => v.atan(),
                    "INT" => v.floor(),
                    "SIGN" => {
                        if v == 0. {
                            0.
                        } else {
                            v.signum()
                        }
                    }
                    "NOT" => f64::from(v == 0.),
                    "IF" => {
                        if !v.is_finite() || !self.eat(',') {
                            return None;
                        }
                        let yes = self.expr(level + 1)?;
                        if !self.eat(',') {
                            return None;
                        }
                        let no = self.expr(level + 1)?;
                        if v != 0. { yes } else { no }
                    }
                    "ATAN2" => {
                        if !self.eat(',') {
                            return None;
                        }
                        let y = self.expr(level + 1)?;
                        if v == 0. && y == 0. { 0. } else { y.atan2(v) }
                    }
                    "MIN" | "MAX" => {
                        let mut r = v;
                        while self.eat(',') {
                            let next = self.expr(level + 1)?;
                            r = if name.eq_ignore_ascii_case("MIN") {
                                r.min(next)
                            } else {
                                r.max(next)
                            };
                        }
                        r
                    }
                    _ => return None,
                };
                return self.eat(')').then_some(v);
            }
            if name.eq_ignore_ascii_case("TRUE") {
                return Some(1.);
            }
            if name.eq_ignore_ascii_case("FALSE") {
                return Some(0.);
            }
            return cell_budget(self.node, self.master, name, self.depth + 1, self.budget);
        }
        let mut len = 0;
        let mut exponent = false;
        for (i, b) in self.rest.bytes().enumerate() {
            if b.is_ascii_digit() || b == b'.' {
                len = i + 1;
                exponent = false;
            } else if b == b'e' || b == b'E' {
                len = i + 1;
                exponent = true;
            } else if exponent && (b == b'+' || b == b'-') {
                len = i + 1;
                exponent = false;
            } else {
                break;
            }
        }
        let mut v = self.rest[..len].parse::<f64>().ok()?;
        self.rest = self.rest[len..].trim_start();
        let n = self
            .rest
            .bytes()
            .take_while(u8::is_ascii_alphabetic)
            .count();
        if n > 0 {
            v *= match self.rest[..n].to_ascii_lowercase().as_str() {
                "in" => 1.,
                "ft" => 12.,
                "mm" => 1. / 25.4,
                "cm" => 1. / 2.54,
                "pt" => 1. / 72.,
                "deg" => std::f64::consts::PI / 180.,
                "rad" => 1.,
                _ => return None,
            };
            self.rest = &self.rest[n..];
        }
        Some(v)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formulas_units_inheritance_and_cycles() {
        let n=super::super::xml::parse(r#"<Shape><Cell N="Width" V="4"/><Cell N="Height" F="GUARD(Width/2 + 72 pt)"/><Cell N="Angle" F="90 deg"/><Cell N="Cycle" F="Cycle+1"/><Cell N="Bad" F="1/0"/></Shape>"#).unwrap();
        assert_eq!(cell(&n, None, "Height", 0), Some(3.));
        assert_eq!(
            cell(&n, None, "Angle", 0),
            Some(std::f64::consts::FRAC_PI_2)
        );
        assert_eq!(cell(&n, None, "Cycle", 0), None);
        assert_eq!(cell(&n, None, "Bad", 0), None);
    }
    #[test]
    fn conditional_geometry_and_inverse_trigonometry() {
        let n = super::super::xml::parse(
            r#"<Shape>
          <Cell N="Width" V="4"/><Cell N="Height" V="2"/>
          <Cell N="X" F="IF(Width&gt;=Height,Width/2,Height/2)"/>
          <Cell N="Y" F="IF(Width&lt;Height,1/0,3)"/>
          <Cell N="Angle" F="ATAN2(0,1)"/>
          <Cell N="Turn" F="PI()/2"/>
          <Cell N="Flag" F="NOT(FALSE)"/>
          <Cell N="Bad" F="IF(1/0,1,2)"/>
        </Shape>"#,
        )
        .unwrap();
        assert_eq!(cell(&n, None, "X", 0), Some(2.));
        assert_eq!(cell(&n, None, "Y", 0), Some(3.));
        assert_eq!(
            cell(&n, None, "Angle", 0),
            Some(std::f64::consts::FRAC_PI_2)
        );
        assert_eq!(cell(&n, None, "Turn", 0), Some(std::f64::consts::FRAC_PI_2));
        assert_eq!(cell(&n, None, "Flag", 0), Some(1.));
        assert_eq!(cell(&n, None, "Bad", 0), None);
    }
}
