//! The small arithmetic of an effect's `derive` formulas: numbers, names, `+ - * /`, brackets and
//! `round floor ceil min max`. A name is looked up by the caller. Twin of `evalExpr` in `effects-engine.cjs`.

/// JavaScript's `Math.round`: halves go up, also for negative numbers.
fn js_round(x: f64) -> f64 {
    let r = x.floor();
    if x - r >= 0.5 { r + 1.0 } else { r }
}

struct Parser<'a> {
    s: &'a [u8],
    src: &'a str,
    i: usize,
    get: &'a mut dyn FnMut(&str) -> Result<f64, String>,
}

impl Parser<'_> {
    fn fail<T>(&self, m: &str) -> Result<T, String> {
        Err(format!("{m} in \"{}\"", self.src))
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && (self.s[self.i] as char).is_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn digits(&self, from: usize) -> usize {
        let mut j = from;
        while j < self.s.len() && self.s[j].is_ascii_digit() {
            j += 1;
        }
        j
    }

    fn primary(&mut self) -> Result<f64, String> {
        self.ws();
        match self.peek() {
            Some(b'(') => {
                self.i += 1;
                let v = self.add()?;
                self.ws();
                if self.peek() != Some(b')') {
                    return self.fail("missing )");
                }
                self.i += 1;
                return Ok(v);
            }
            Some(b'-') => {
                self.i += 1;
                return Ok(-self.primary()?);
            }
            _ => {}
        }
        // a number: [0-9]*\.?[0-9]+
        let int_end = self.digits(self.i);
        let number_end = if self.s.get(int_end) == Some(&b'.') && self.digits(int_end + 1) > int_end + 1 {
            Some(self.digits(int_end + 1))
        } else if int_end > self.i {
            Some(int_end)
        } else {
            None
        };
        if let Some(end) = number_end {
            let n: f64 = self.src[self.i..end].parse().unwrap_or(f64::NAN);
            self.i = end;
            return Ok(n);
        }
        // a name: [A-Za-z_]\w*
        let start = self.i;
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {}
            Some(c) => return self.fail(&format!("unexpected {:?}", (c as char).to_string())),
            None => return self.fail("unexpected \"end\""),
        }
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
            self.i += 1;
        }
        let name = &self.src[start..self.i];
        self.ws();
        if self.peek() == Some(b'(') {
            self.i += 1;
            let mut args = Vec::new();
            self.ws();
            if self.peek() != Some(b')') {
                loop {
                    args.push(self.add()?);
                    self.ws();
                    if self.peek() == Some(b',') {
                        self.i += 1;
                        continue;
                    }
                    break;
                }
            }
            if self.peek() != Some(b')') {
                return self.fail("missing )");
            }
            self.i += 1;
            return match name {
                "round" => Ok(js_round(args.first().copied().unwrap_or(f64::NAN))),
                "floor" => Ok(args.first().copied().unwrap_or(f64::NAN).floor()),
                "ceil" => Ok(args.first().copied().unwrap_or(f64::NAN).ceil()),
                "min" => Ok(args.iter().copied().fold(f64::INFINITY, |a, b| if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) })),
                "max" => Ok(args.iter().copied().fold(f64::NEG_INFINITY, |a, b| if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) })),
                _ => self.fail(&format!("unknown function {name}")),
            };
        }
        (self.get)(name)
    }

    fn mul(&mut self) -> Result<f64, String> {
        let mut v = self.primary()?;
        loop {
            self.ws();
            match self.peek() {
                Some(b'*') => {
                    self.i += 1;
                    v *= self.primary()?;
                }
                Some(b'/') => {
                    self.i += 1;
                    v /= self.primary()?;
                }
                _ => return Ok(v),
            }
        }
    }

    fn add(&mut self) -> Result<f64, String> {
        let mut v = self.mul()?;
        loop {
            self.ws();
            match self.peek() {
                Some(b'+') => {
                    self.i += 1;
                    v += self.mul()?;
                }
                Some(b'-') => {
                    self.i += 1;
                    v -= self.mul()?;
                }
                _ => return Ok(v),
            }
        }
    }
}

/// Evaluates `src`; `get_num` gives the number a name stands for.
pub fn eval_expr(src: &str, get_num: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<f64, String> {
    let mut p = Parser { s: src.as_bytes(), src, i: 0, get: get_num };
    let v = p.add()?;
    p.ws();
    if p.i < p.s.len() {
        let c = src[p.i..].chars().next().unwrap_or(' ');
        return p.fail(&format!("unexpected {:?}", c.to_string()));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(s: &str) -> Result<f64, String> {
        eval_expr(s, &mut |n| match n {
            "height" => Ok(1080.0),
            "ar" => Ok(16.0 / 9.0),
            _ => Err(format!("unknown variable {n}")),
        })
    }

    #[test]
    fn arithmetic_and_functions() {
        assert_eq!(eval("2*round(height/2)").unwrap(), 1080.0);
        assert_eq!(eval("2*round(height*ar/2)").unwrap(), 1920.0);
        assert_eq!(eval("1 + 2 * 3 - 4 / 2").unwrap(), 5.0);
        assert_eq!(eval("-(2+3)").unwrap(), -5.0);
        assert_eq!(eval("min(3, 1, 2) + max(1, 5)").unwrap(), 6.0);
        assert_eq!(eval(".5 + 1.25").unwrap(), 1.75);
        assert_eq!(eval("round(2.5)").unwrap(), 3.0);
        assert_eq!(eval("round(-2.5)").unwrap(), -2.0);
    }

    #[test]
    fn mistakes_are_named() {
        assert!(eval("2 +").unwrap_err().contains("unexpected"));
        assert!(eval("(2").unwrap_err().contains("missing )"));
        assert!(eval("nope").unwrap_err().contains("unknown variable"));
        assert!(eval("sqrt(4)").unwrap_err().contains("unknown function sqrt"));
        assert!(eval("2 3").unwrap_err().contains("unexpected"));
    }
}
