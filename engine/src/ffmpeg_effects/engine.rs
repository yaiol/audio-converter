//! The effects engine: from an effect and the values of its controls to ffmpeg text. Twin of `effects-engine.cjs`:
//! the same steps in the same order, so the two write the same text for the same values (`check/` runs thousands of cases through both).
//! Read the JavaScript's header for what an effect is; this file does not repeat it.

use super::expr::eval_expr;
use super::model::*;
use super::value::V;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub type Filters = BTreeMap<String, Filter>;
/// The values of an effect's controls, by control id. A control with no entry has its default.
pub type Values = HashMap<String, V>;
/// What the page or the job knows that the effect may use: `input` (the source file) and `font` (a font file).
pub type Ctx = HashMap<String, String>;

const CTX: [&str; 4] = ["input", "font", "name", "duration"];

/// A choice of a control as the user sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub value: V,
    pub label: String,
    /// For a flag: the other names ffmpeg knows it by (the same meaning, written differently).
    pub also: Vec<String>,
    /// Variables this choice sets.
    pub set: BTreeMap<String, String>,
    /// The filters this choice stands for.
    pub steps: Option<Vec<Step>>,
}

/// A control, resolved: what the form shows for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    /// Where it comes from: `effect` (an own control) or `option filter.name`.
    pub origin: String,
    pub id: String,
    pub label: String,
    pub help: Option<String>,
    /// integer, decimal, switch, list, checkboxes, colour, duration, size, fraction, format, file, saveFile, folder, text ...
    pub control: String,
    pub choices: Vec<Choice>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub soft: Option<Soft>,
    pub unit: Option<String>,
    pub step: Option<f64>,
    pub required: bool,
    pub default: Option<V>,
    pub ext: Option<String>,
    pub example: Option<String>,
}

fn kind(control: &str) -> &str {
    match control {
        "integer" | "decimal" | "switch" | "colour" | "duration" | "size" | "fraction" | "checkboxes" | "format" | "list" => control,
        _ => "text",
    }
}

pub fn find_option<'a>(filters: &'a Filters, filter: &str, name: &str) -> Option<&'a FOption> {
    filters.get(filter)?.options.iter().find(|o| o.name == name || o.short.iter().any(|s| s == name))
}

/// A filter seen as an effect: every option is a control, and one step sets them all.
pub fn implicit(name: &str, filters: &Filters) -> Option<Effect> {
    let rec = filters.get(name)?;
    let controls: Vec<ControlDef> = rec
        .options
        .iter()
        .filter(|o| o.alias_of.is_none())
        .map(|o| ControlDef { id: o.name.clone(), bind: Some(format!("{name}.{}", o.name)), ..Default::default() })
        .collect();
    let with = Ordered(controls.iter().map(|c| (c.id.clone(), With::Control(c.id.clone()))).collect());
    let step = Some(vec![Step { filter: Some(name.to_string()), choice: None, with }]);
    let sound = rec.works == "audio";
    Some(Effect {
        implicit: true,
        name: name.to_string(),
        plain: Some(rec.plain.clone().unwrap_or_else(|| name.to_string())),
        controls,
        // a filter that works on sound only is the sound's chain, not the picture's
        steps: if sound { None } else { step.clone() },
        audio: if sound { step } else { None },
        ..Default::default()
    })
}

fn is_unset_constant(s: &str) -> bool {
    // INT_MAX, UINT64_MIN, I64_MAX, DBL_MIN, FLT_MAX ...
    let Some((head, tail)) = s.rsplit_once('_') else { return false };
    matches!(tail, "MAX" | "MIN") && matches!(head, "INT" | "UINT" | "INT64" | "UINT64" | "I64" | "DBL" | "FLT")
}

/// ffmpeg writes "not set" as a C constant: a form shows that as an empty field, and an empty field writes nothing.
fn unset_default(o: &FOption, x: &V) -> bool {
    is_unset_constant(&x.js_string()) || (o.control == "duration" && x.to_number() == -0.000001)
}

/// The choices of an option as a form offers them: our labels, ffmpeg's aliases folded into one, a refused one left out.
pub fn choices_of_option(o: &FOption) -> Vec<Choice> {
    let plain = |value: V, label: String| Choice { value, label, also: Vec::new(), set: BTreeMap::new(), steps: None };
    if let Some(a) = &o.as_ {
        if let Some(cs) = &a.choices {
            return cs
                .iter()
                .map(|c| {
                    let v = c.value.as_ref().map(V::from_json).unwrap_or(V::Null);
                    let label = c.plain.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| v.js_string());
                    plain(v, label)
                })
                .collect();
        }
    }
    let mut out: Vec<Choice> = Vec::new();
    let all = o.choices.as_deref().unwrap_or(&[]);
    if o.control == "checkboxes" {
        let mut by_help: HashMap<&str, usize> = HashMap::new();
        for c in all {
            if !c.help.is_empty() {
                if let Some(&first) = by_help.get(c.help.as_str()) {
                    out[first].also.push(c.name.clone());
                    continue;
                }
            }
            out.push(plain(V::Str(c.name.clone()), c.plain.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| c.name.clone())));
            if !c.help.is_empty() {
                by_help.insert(c.help.as_str(), out.len() - 1);
            }
        }
        return out;
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for c in all {
        if c.broken.is_some() {
            continue;
        }
        let key = match &c.value {
            Some(v) => format!("v{}", V::from_json(v).js_string()),
            None => format!("n{}", c.name),
        };
        if !seen.insert(key) {
            continue;
        }
        out.push(plain(V::Str(c.name.clone()), c.plain.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| c.name.clone())));
    }
    out
}

fn bound_control(c: &ControlDef, filters: &Filters) -> Result<Control, String> {
    let bind = c.bind.as_deref().unwrap_or_default();
    let (f, on) = bind.split_once('.').map(|(a, b)| (a, b.split('.').next().unwrap_or(b))).unwrap_or((bind, ""));
    let o = find_option(filters, f, on).ok_or_else(|| format!("{}: {} is not an option", c.id, bind))?;
    let control = match o.as_.as_ref().and_then(|a| a.control.clone()).filter(|s| !s.is_empty()) {
        Some(x) => x,
        None if o.choices.is_some() => (if o.control == "checkboxes" { "checkboxes" } else { "list" }).to_string(),
        None => kind(&o.control).to_string(),
    };
    let default_value = c.default.as_ref().or(o.default.as_ref()).map(V::from_json);
    let default = default_value.map(|d| if unset_default(o, &d) { V::Str(String::new()) } else { d });
    let pick = |a: &Option<String>, b: &Option<String>, c2: &Option<String>| a.clone().filter(|s| !s.is_empty()).or_else(|| b.clone().filter(|s| !s.is_empty())).or_else(|| c2.clone());
    Ok(Control {
        origin: format!("option {bind}"),
        id: c.id.clone(),
        label: c.label.clone().filter(|s| !s.is_empty()).or_else(|| o.plain.clone().filter(|s| !s.is_empty())).unwrap_or_else(|| o.name.clone()),
        help: pick(&c.help, &o.what, &o.help),
        choices: if control == "list" || control == "checkboxes" { choices_of_option(o) } else { Vec::new() },
        control,
        min: o.min,
        max: o.max,
        soft: o.soft,
        unit: o.unit.clone().filter(|s| !s.is_empty()),
        step: o.step.filter(|x| *x != 0.0),
        required: c.required || o.required,
        default,
        ext: c.ext.clone().filter(|s| !s.is_empty()),
        example: c.example.clone().filter(|s| !s.is_empty()),
    })
}

/// The controls of an effect, resolved against the filters.
pub fn resolve_controls(effect: &Effect, filters: &Filters) -> Result<Vec<Control>, String> {
    effect
        .controls
        .iter()
        .map(|c| {
            if c.bind.is_some() {
                return bound_control(c, filters);
            }
            Ok(Control {
                origin: "effect".into(),
                id: c.id.clone(),
                label: c.label.clone().unwrap_or_default(),
                help: c.help.clone(),
                control: c.control.clone().unwrap_or_default(),
                choices: c
                    .choices
                    .iter()
                    .flatten()
                    .map(|ch| Choice {
                        value: ch.value.as_ref().map(V::from_json).unwrap_or(V::Null),
                        label: ch.label.clone().unwrap_or_default(),
                        also: Vec::new(),
                        set: ch.set.clone().unwrap_or_default(),
                        steps: ch.steps.clone(),
                    })
                    .collect(),
                min: c.min,
                max: c.max,
                soft: None,
                unit: c.unit.clone(),
                step: c.step,
                required: c.required,
                default: c.default.as_ref().map(V::from_json),
                ext: c.ext.clone(),
                example: c.example.clone(),
            })
        })
        .collect()
}

/// The value every control has until the user changes it.
pub fn defaults(effect: &Effect, filters: &Filters) -> Result<Values, String> {
    let mut v = Values::new();
    for c in resolve_controls(effect, filters)? {
        if let Some(d) = c.default {
            v.insert(c.id, d);
        }
    }
    Ok(v)
}

// ---- variables

struct Resolver<'a> {
    raw: HashMap<String, V>,
    lazy: BTreeMap<String, String>,
    derive: &'a BTreeMap<String, String>,
    cache: RefCell<HashMap<String, V>>,
}

impl Resolver<'_> {
    fn get(&self, name: &str, stack: &[String]) -> Result<V, String> {
        if let Some(v) = self.raw.get(name) {
            return Ok(v.clone());
        }
        if let Some(v) = self.cache.borrow().get(name) {
            return Ok(v.clone());
        }
        if stack.iter().any(|s| s == name) {
            return Err(format!("a variable refers to itself: {name}"));
        }
        let mut next = stack.to_vec();
        next.push(name.to_string());
        let out = if let Some(tpl) = self.lazy.get(name) {
            V::Str(expand(tpl, &mut |n| self.get(n, &next))?)
        } else if let Some(f) = self.derive.get(name) {
            V::Num(eval_expr(f, &mut |n| self.num(n, &next))?)
        } else {
            return Err(format!("unknown variable {name}"));
        };
        self.cache.borrow_mut().insert(name.to_string(), out.clone());
        Ok(out)
    }

    fn num(&self, name: &str, stack: &[String]) -> Result<f64, String> {
        match self.get(name, stack)? {
            V::Num(n) => Ok(n),
            other => eval_expr(&other.js_string(), &mut |m| self.num(m, stack)),
        }
    }
}

fn resolver<'a>(effect: &'a Effect, filters: &Filters, values: &Values, ctx: &Ctx) -> Result<(Resolver<'a>, Vec<Control>), String> {
    let controls = resolve_controls(effect, filters)?;
    let mut raw: HashMap<String, V> = ctx.iter().map(|(k, v)| (k.clone(), V::Str(v.clone()))).collect();
    let mut lazy: BTreeMap<String, String> = BTreeMap::new();
    for c in &controls {
        let v = values.get(&c.id).cloned().or_else(|| c.default.clone());
        let stored = if c.control == "switch" {
            V::Num(if v.as_ref().is_some_and(V::truthy) { 1.0 } else { 0.0 })
        } else {
            v.clone().unwrap_or(V::Str(String::new()))
        };
        raw.insert(c.id.clone(), stored);
        if c.control == "list" {
            let want = v.as_ref().map(V::js_string).unwrap_or_else(|| "undefined".into());
            if let Some(ch) = c.choices.iter().find(|x| x.value.js_string() == want) {
                lazy.extend(ch.set.clone());
            }
        }
    }
    Ok((Resolver { raw, lazy, derive: &effect.derive, cache: RefCell::new(HashMap::new()) }, controls))
}

/// Replaces `{name}` by the variable's text.
fn expand(tpl: &str, get: &mut dyn FnMut(&str) -> Result<V, String>) -> Result<String, String> {
    let b = tpl.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'{' {
            let start = i + 1;
            let mut j = start;
            if j < b.len() && (b[j].is_ascii_alphabetic() || b[j] == b'_') {
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                    j += 1;
                }
                if j < b.len() && b[j] == b'}' {
                    out.push_str(&get(&tpl[start..j])?.js_string());
                    i = j + 1;
                    continue;
                }
            }
        }
        // copy one whole character
        let ch = tpl[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    Ok(out)
}

// ---- time, flags

/// What a time field holds, read as ffmpeg reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dur {
    Empty,
    /// Not a time ffmpeg reads.
    Bad,
    Seconds(f64),
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
}

/// `\d{1,2}` or `\d{1,2}.\d+`
fn seconds_part(s: &str) -> Option<f64> {
    let (int, frac) = match s.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    if !(1..=2).contains(&int.len()) || !all_digits(int) || frac.is_some_and(|f| !all_digits(f)) {
        return None;
    }
    s.parse().ok()
}

/// A time as ffmpeg reads it: seconds (`90`, `1.5`, `200ms`, `200000us`) or `[HH:]MM:SS[.m]`.
pub fn dur_seconds(s: &str) -> Dur {
    let s = s.trim();
    if s.is_empty() {
        return Dur::Empty;
    }
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let sign = if neg { -1.0 } else { 1.0 };
    // clock time
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.len() == 2 || parts.len() == 3 {
        let (h, m, sec) = if parts.len() == 3 { (Some(parts[0]), parts[1], parts[2]) } else { (None, parts[0], parts[1]) };
        let m_ok = (1..=2).contains(&m.len()) && all_digits(m);
        let h_ok = h.is_none_or(all_digits);
        if m_ok && h_ok {
            if let Some(sv) = seconds_part(sec) {
                let mv: f64 = m.parse().unwrap_or(f64::NAN);
                if (h.is_none() || mv < 60.0) && sv < 60.0 {
                    let hv: f64 = h.map(|x| x.parse().unwrap_or(f64::NAN)).unwrap_or(0.0);
                    return Dur::Seconds(sign * (hv * 3600.0 + mv * 60.0 + sv));
                }
            }
        }
    }
    // seconds, with a unit
    let (num, div) = if let Some(n) = rest.strip_suffix("ms") {
        (n, 1e3)
    } else if let Some(n) = rest.strip_suffix("us") {
        (n, 1e6)
    } else if let Some(n) = rest.strip_suffix('s') {
        (n, 1.0)
    } else {
        (rest, 1.0)
    };
    let (int, frac) = match num.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (num, None),
    };
    if all_digits(int) && frac.is_none_or(all_digits) {
        if let Ok(n) = num.parse::<f64>() {
            return Dur::Seconds(sign * n / div);
        }
    }
    Dur::Bad
}

const FLAG_SHORTCUTS: [&str; 2] = ["all", "none"];

/// The names of the flags of a flags control: `all` and `none` are shortcuts, not flags.
pub fn flag_names(choices: &[Choice]) -> Vec<String> {
    choices.iter().map(|c| c.value.js_string()).filter(|n| !FLAG_SHORTCUTS.contains(&n.as_str())).collect()
}

/// The flags a written value switches on; `None` when it cannot tell (a hex mask, a name ffmpeg lists nowhere).
pub fn flag_set(choices: &[Choice], text: &str) -> Option<BTreeSet<String>> {
    let names = flag_names(choices);
    let tokens: Vec<&str> = text.split('+').map(str::trim).filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() || (tokens.len() == 1 && (tokens[0] == "0" || tokens[0] == "none")) {
        return Some(BTreeSet::new());
    }
    if tokens.contains(&"all") {
        return Some(names.into_iter().collect());
    }
    let canon = |t: &str| choices.iter().find(|c| c.value.js_string() == t || c.also.iter().any(|a| a == t)).map(|c| c.value.js_string());
    tokens.iter().map(|t| canon(t)).collect()
}

/// A set of flags as ffmpeg is given it: names joined by `+`.
pub fn flag_text(choices: &[Choice], set: &BTreeSet<String>) -> String {
    let on: Vec<String> = flag_names(choices).into_iter().filter(|n| set.contains(n)).collect();
    if !on.is_empty() {
        return on.join("+");
    }
    if choices.iter().any(|c| c.value.js_string() == "none") { "none".into() } else { "0".into() }
}

// ---- steps to text

/// How a text is written into the filter text, by the option's `escape`.
pub fn escape(kind: &str, v: &str, ctx: &dyn Fn(&str) -> String) -> Option<String> {
    match kind {
        // the same, for text that may hold tokens: {name} the file's name (ctx `name`), {elapsed} the playing time and {remaining} the
        // time left (ctx `duration`, seconds, 0 = unknown), both as minutes:seconds that follow the playing time. A `{` that starts no token is text.
        "drawtext-template" => {
            let lit = |s: &str| s.replace('\'', "\u{2019}").replace('\\', "\\\\").replace('%', "\\%").replace('\\', "\\\\").replace(':', "\\:");
            let mmss = |e: &str| format!("%{{eif:trunc(({e})/60):d:2}}:%{{eif:trunc(mod(({e}),60)):d:2}}").replace(':', "\\:");
            let duration = format!("{:.3}", ctx("duration").trim().parse::<f64>().ok().filter(|d| d.is_finite()).unwrap_or(0.0));
            let mut out = String::new();
            let mut rest = v;
            while let Some(open) = rest.find('{') {
                out.push_str(&lit(&rest[..open]));
                let after = &rest[open..];
                let (text, len) = if after.starts_with("{name}") {
                    (lit(&ctx("name")), 6)
                } else if after.starts_with("{elapsed}") {
                    (mmss("t"), 9)
                } else if after.starts_with("{remaining}") {
                    (mmss(&format!("max(0,{duration}-t)")), 11)
                } else {
                    ("{".to_string(), 1)
                };
                out.push_str(&text);
                rest = &after[len..];
            }
            Some(format!("'{out}{}'", lit(rest)))
        }
        // a file path: forward slashes, the drive colon escaped, quoted
        "path" => Some(format!("'{}'", v.replace('\\', "/").replace(':', "\\:").replace('\'', "'\\''"))),
        "quoted" => Some(quoted(v)),
        // text drawn by drawtext passes three readers: drawtext's own expansion, the filter's option parser, the graph parser
        "drawtext" => {
            let a = v.replace('\'', "\u{2019}").replace('\\', "\\\\").replace('%', "\\%");
            let b = a.replace('\\', "\\\\").replace(':', "\\:");
            Some(format!("'{b}'"))
        }
        _ => None,
    }
}

fn quoted(v: &str) -> String {
    format!("'{}'", v.replace('\'', "'\\''"))
}

fn needs_quotes(v: &str) -> bool {
    v.chars().any(|c| matches!(c, ',' | ';' | ':' | '[' | ']' | '\'' | '=' | '\\') || c.is_whitespace())
}

fn format_value(o: Option<&FOption>, v: &V, ctx: &dyn Fn(&str) -> String) -> String {
    if let V::Bool(b) = v {
        return if *b { "1" } else { "0" }.into();
    }
    let mut s = v.js_string();
    if let Some(o) = o {
        if o.control == "duration" && s.contains(':') {
            if let Dur::Seconds(x) = dur_seconds(&s) {
                s = V::Num(x).js_string();
            }
        }
        if let Some(e) = o.escape.as_deref().and_then(|k| escape(k, &s, ctx)) {
            return e;
        }
    }
    if needs_quotes(&s) { quoted(&s) } else { s }
}

fn same_as_default(o: Option<&FOption>, v: &V) -> bool {
    let Some(o) = o else { return false };
    let Some(def) = o.default.as_ref().map(V::from_json) else { return false };
    if unset_default(o, &def) {
        return matches!(v, V::Str(s) if s.is_empty()) || v.js_string() == def.js_string();
    }
    if o.control == "checkboxes" && o.choices.is_some() {
        let choices = choices_of_option(o);
        if let (Some(a), Some(b)) = (flag_set(&choices, &def.js_string()), flag_set(&choices, &v.js_string())) {
            return a == b;
        }
    }
    if o.control == "switch" {
        let on = matches!(v, V::Bool(true)) || matches!(v, V::Num(n) if *n == 1.0) || matches!(v, V::Str(s) if s == "1");
        return on == def.truthy();
    }
    v.js_string() == def.js_string()
}

fn step_text(step: &Step, get: &mut dyn FnMut(&str) -> Result<V, String>, filters: &Filters, only_changed: bool) -> Result<String, String> {
    let filter = step.filter.as_deref().unwrap_or_default();
    let mut parts: Vec<String> = Vec::new();
    // the context values escapes may use (`name`, `duration`); one that was not given is empty
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for k in ["name", "duration"] {
        seen.insert(k.to_string(), get(k).map(|v| v.js_string()).unwrap_or_default());
    }
    let ctx_of = |k: &str| seen.get(k).cloned().unwrap_or_default();
    for (name, w) in &step.with.0 {
        let val = match w {
            With::Literal(v) => v.clone(),
            With::Control(id) => get(id)?,
            With::Expr(e) => V::Str(expand(e, get)?),
            With::Ctx(c) => get(c)?,
            With::Unknown => return Err(format!("{filter}.{name}: unknown value form")),
        };
        if matches!(&val, V::Null) || matches!(&val, V::Str(s) if s.is_empty()) {
            continue;
        }
        let o = find_option(filters, filter, name);
        if only_changed && same_as_default(o, &val) {
            continue;
        }
        parts.push(format!("{name}={}", format_value(o, &val, &ctx_of)));
    }
    Ok(if parts.is_empty() { filter.to_string() } else { format!("{filter}={}", parts.join(":")) })
}

fn chain_text(
    steps: &[Step],
    effect: &Effect,
    get: &mut dyn FnMut(&str) -> Result<V, String>,
    filters: &Filters,
    only_changed: bool,
) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for s in steps {
        if let Some(id) = &s.choice {
            let controls = resolve_controls(effect, filters)?;
            let c = controls.iter().find(|x| &x.id == id).ok_or_else(|| format!("{id}: there is no such control"))?;
            let want = get(id)?.js_string();
            let ch = c.choices.iter().find(|x| x.value.js_string() == want);
            let steps = ch.and_then(|c| c.steps.as_ref()).ok_or_else(|| format!("{id}: the chosen choice has no steps"))?;
            out.extend(chain_text(steps, effect, get, filters, only_changed)?);
        } else {
            out.push(step_text(s, get, filters, only_changed)?);
        }
    }
    Ok(out)
}

/// What an effect writes, in parts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Built {
    pub video: Option<String>,
    pub audio: Option<String>,
    pub graph: Option<String>,
    /// The same graph piece by piece (labels apart from the filters), for `compose`.
    pub chains: Vec<BuiltChain>,
    pub inputs: Vec<String>,
}

/// A piece of a filter graph: the labels it reads, its filters as text, the labels it makes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BuiltChain {
    pub from: Vec<String>,
    pub text: String,
    pub to: Vec<String>,
}

pub fn build(effect: &Effect, filters: &Filters, values: &Values, ctx: &Ctx) -> Result<Built, String> {
    let (r, _) = resolver(effect, filters, values, ctx)?;
    let mut get = |n: &str| r.get(n, &[]);
    let only = effect.implicit;
    let label = |a: &[String]| a.iter().map(|l| format!("[{l}]")).collect::<String>();
    let video = match &effect.steps {
        Some(s) => Some(chain_text(s, effect, &mut get, filters, only)?.join(",")),
        None => None,
    };
    let audio = match &effect.audio {
        Some(s) => Some(chain_text(s, effect, &mut get, filters, only)?.join(",")),
        None => None,
    };
    let mut chains = Vec::new();
    let graph = match &effect.graph {
        Some(g) => {
            let mut pieces = Vec::new();
            for c in g {
                let text = chain_text(&c.steps, effect, &mut get, filters, only)?.join(",");
                pieces.push(format!("{}{}{}", label(&c.from), text, label(&c.to)));
                chains.push(BuiltChain { from: c.from.clone(), text, to: c.to.clone() });
            }
            Some(pieces.join(";"))
        }
        None => None,
    };
    let mut inputs = Vec::new();
    for i in &effect.inputs {
        inputs.push(get(&i.control)?.js_string());
    }
    Ok(Built { video, audio, graph, chains, inputs })
}

/// The arguments after the main input: extra inputs, then the filters.
pub fn args(effect: &Effect, filters: &Filters, values: &Values, ctx: &Ctx) -> Result<Vec<String>, String> {
    let b = build(effect, filters, values, ctx)?;
    let mut a: Vec<String> = Vec::new();
    for i in &b.inputs {
        a.push("-i".into());
        a.push(i.clone());
    }
    if let Some(g) = b.graph.as_ref().filter(|g| !g.is_empty()) {
        a.extend(["-filter_complex".into(), g.clone(), "-map".into(), "[outv]".into()]);
    } else if let Some(v) = b.video.as_ref().filter(|v| !v.is_empty()) {
        a.extend(["-vf".into(), v.clone()]);
    }
    if let Some(v) = b.audio.as_ref().filter(|v| !v.is_empty()) {
        a.extend(["-af".into(), v.clone()]);
    }
    Ok(a)
}

/// Every filter an effect is made of, in order of appearance.
pub fn used_filters(effect: &Effect) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |steps: &[Step]| {
        for s in steps {
            if let Some(f) = &s.filter {
                if !out.contains(f) {
                    out.push(f.clone());
                }
            }
        }
    };
    add(effect.steps.as_deref().unwrap_or(&[]));
    add(effect.audio.as_deref().unwrap_or(&[]));
    for c in effect.graph.iter().flatten() {
        add(&c.steps);
    }
    for c in &effect.controls {
        for ch in c.choices.iter().flatten() {
            add(ch.steps.as_deref().unwrap_or(&[]));
        }
    }
    out
}

fn expr_variables(e: &str) -> Vec<String> {
    // `{name}` with name made of word characters (the check reads digits too; a variable starts with a letter)
    let b = e.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'{' {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            if j > i + 1 && j < b.len() && b[j] == b'}' {
                out.push(e[i + 1..j].to_string());
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Is the effect valid against the filters? Returns the problems, none when it is.
pub fn check(effect: &Effect, filters: &Filters) -> Vec<String> {
    let mut bad: Vec<String> = Vec::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut vars: BTreeSet<String> = CTX.iter().map(|s| s.to_string()).collect();
    for c in &effect.controls {
        if !ids.insert(c.id.clone()) {
            bad.push(format!("control {} appears twice", c.id));
        }
        vars.insert(c.id.clone());
        if let Some(bind) = &c.bind {
            let (f, on) = bind.split_once('.').unwrap_or((bind, ""));
            if find_option(filters, f, on).is_none() {
                bad.push(format!("{}: {} is not an option", c.id, bind));
            }
            for (k, present) in [("min", c.min.is_some()), ("max", c.max.is_some()), ("choices", c.choices.is_some()), ("control", c.control.is_some())] {
                if present {
                    bad.push(format!("{}: a control bound to an option takes its {k} from the option and cannot change it", c.id));
                }
            }
        }
        for ch in c.choices.iter().flatten() {
            vars.extend(ch.set.iter().flat_map(|s| s.keys().cloned()));
        }
    }
    vars.extend(effect.derive.keys().cloned());

    let ok_var = |bad: &mut Vec<String>, n: &str, wher: &str| {
        if !vars.contains(n) {
            bad.push(format!("{wher}: unknown variable {n}"));
        }
    };
    let check_steps = |bad: &mut Vec<String>, steps: &[Step], wher: &str| {
        for s in steps {
            if let Some(id) = &s.choice {
                let ok = effect.controls.iter().find(|x| &x.id == id).is_some_and(|c| c.choices.iter().flatten().all(|ch| ch.steps.is_some()));
                if !ok {
                    bad.push(format!("{wher}: choice {id} needs a control whose every choice has steps"));
                }
                continue;
            }
            let fname = s.filter.as_deref().unwrap_or_default();
            if !filters.contains_key(fname) {
                bad.push(format!("{wher}: there is no filter {fname}"));
                continue;
            }
            for (name, w) in &s.with.0 {
                let Some(o) = find_option(filters, fname, name) else {
                    bad.push(format!("{wher}: {fname} has no option {name}"));
                    continue;
                };
                match w {
                    With::Control(id) => {
                        if !ids.contains(id) {
                            bad.push(format!("{wher}: {fname}.{name} reads control {id}, which does not exist"));
                        }
                    }
                    With::Expr(e) => {
                        for m in expr_variables(e) {
                            ok_var(bad, &m, &format!("{wher}: {fname}.{name}"));
                        }
                    }
                    With::Ctx(c) => {
                        if !CTX.contains(&c.as_str()) {
                            bad.push(format!("{wher}: unknown context value {c}"));
                        }
                    }
                    With::Unknown => {}
                    With::Literal(v) => {
                        if let Some(cs) = &o.choices {
                            if !cs.iter().any(|c| c.name == v.js_string()) {
                                bad.push(format!("{wher}: \"{}\" is not one of ffmpeg's choices for {fname}.{name}", v.js_string()));
                            }
                        }
                        if let (V::Num(n), Some(min)) = (v, o.min) {
                            if *n < min || o.max.is_some_and(|max| *n > max) {
                                bad.push(format!("{wher}: {} is outside ffmpeg's range for {fname}.{name}", v.js_string()));
                            }
                        }
                    }
                }
            }
        }
    };
    check_steps(&mut bad, effect.steps.as_deref().unwrap_or(&[]), "steps");
    check_steps(&mut bad, effect.audio.as_deref().unwrap_or(&[]), "audio");
    for (i, c) in effect.graph.iter().flatten().enumerate() {
        check_steps(&mut bad, &c.steps, &format!("graph {}", i + 1));
    }
    for c in &effect.controls {
        for ch in c.choices.iter().flatten() {
            let value = ch.value.as_ref().map(|v| V::from_json(v).js_string()).unwrap_or_default();
            check_steps(&mut bad, ch.steps.as_deref().unwrap_or(&[]), &format!("choice {value}"));
        }
    }
    for (k, f) in &effect.derive {
        let mut names: Vec<String> = Vec::new();
        let r = eval_expr(f, &mut |n| {
            names.push(n.to_string());
            Ok(1.0)
        });
        for n in &names {
            ok_var(&mut bad, n, &format!("derive {k}"));
        }
        if let Err(e) = r {
            bad.push(format!("derive {k}: {e}"));
        }
    }
    for i in &effect.inputs {
        if !ids.contains(&i.control) {
            bad.push(format!("inputs: control {} does not exist", i.control));
        }
    }
    let used = used_filters(effect);
    for f in &effect.covers {
        if !used.contains(f) {
            bad.push(format!("covers {f}, which the effect does not use"));
        }
    }
    bad
}
