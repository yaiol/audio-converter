//! A value of a control, as the JavaScript engine holds it (`effects-engine.cjs`): nothing, a boolean, a number or a text.
//! `js_string` is JavaScript's `String(v)`: the ffmpeg text a number becomes must be the same digits the other engine writes.

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum V {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
}

impl V {
    pub fn from_json(v: &Value) -> V {
        match v {
            Value::Null => V::Null,
            Value::Bool(b) => V::Bool(*b),
            Value::Number(n) => V::Num(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => V::Str(s.clone()),
            // an array or an object is never a value of a control
            _ => V::Null,
        }
    }

    /// The value as JSON, to be kept in a settings file (`from_json` reads it back). A number that is not finite is `null`.
    pub fn to_json(&self) -> Value {
        match self {
            V::Null => Value::Null,
            V::Bool(b) => Value::Bool(*b),
            V::Num(n) => serde_json::Number::from_f64(*n).map(Value::Number).unwrap_or(Value::Null),
            V::Str(s) => Value::String(s.clone()),
        }
    }

    pub fn str(s: &str) -> V {
        V::Str(s.to_string())
    }

    /// JavaScript's `String(v)`.
    pub fn js_string(&self) -> String {
        match self {
            V::Null => "null".into(),
            V::Bool(b) => b.to_string(),
            V::Num(n) => js_number_string(*n),
            V::Str(s) => s.clone(),
        }
    }

    /// JavaScript's truthiness: `""`, `0`, `NaN`, `null` and `false` are false, every other text (`"0"` and `"false"` too) is true.
    pub fn truthy(&self) -> bool {
        match self {
            V::Null => false,
            V::Bool(b) => *b,
            V::Num(n) => *n != 0.0 && !n.is_nan(),
            V::Str(s) => !s.is_empty(),
        }
    }

    /// JavaScript's `Number(v)`.
    pub fn to_number(&self) -> f64 {
        match self {
            V::Null => 0.0,
            V::Bool(b) => f64::from(u8::from(*b)),
            V::Num(n) => *n,
            V::Str(s) => {
                let t = s.trim();
                if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) }
            }
        }
    }
}

impl From<f64> for V {
    fn from(n: f64) -> V {
        V::Num(n)
    }
}

impl From<&str> for V {
    fn from(s: &str) -> V {
        V::Str(s.to_string())
    }
}

impl From<bool> for V {
    fn from(b: bool) -> V {
        V::Bool(b)
    }
}

/// JavaScript's `String(number)`: the shortest digits that read back the same, in the layout the language uses
/// (no exponent between 1e-6 and 1e21; `1e-7`, `1e+21` outside).
pub fn js_number_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if x == 0.0 {
        return "0".into();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    // `{:e}` is the shortest round-trip form: "1.2345e-7"
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp + 1; // the position of the decimal point, counted from the first digit
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign_e = if e < 0 { "-" } else { "+" };
        if k == 1 { format!("{digits}e{sign_e}{}", e.abs()) } else { format!("{}.{}e{sign_e}{}", &digits[..1], &digits[1..], e.abs()) }
    };
    format!("{sign}{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_as_javascript_writes_them() {
        for (x, s) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-3.0, "-3"),
            (1.5, "1.5"),
            (0.1, "0.1"),
            (6.283185, "6.283185"),
            (100000.0, "100000"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (1.5e22, "1.5e+22"),
            (0.000001, "0.000001"),
            (0.0000001, "1e-7"),
            (-1.25e-9, "-1.25e-9"),
            (123456.789, "123456.789"),
            (0.00001, "0.00001"),
        ] {
            assert_eq!(js_number_string(x), s, "{x}");
        }
    }

    #[test]
    fn truthiness_follows_javascript() {
        assert!(V::str("0").truthy());
        assert!(V::str("false").truthy());
        assert!(!V::str("").truthy());
        assert!(!V::Num(0.0).truthy());
        assert!(!V::Null.truthy());
        assert!(V::Num(2.0).truthy());
    }
}
