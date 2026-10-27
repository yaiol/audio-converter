//! `effects.json` as Rust types. Only what the engine and the form read is modelled; every other field of the file is ignored.
//! The JSON is the contract (see ARCH-ffmpeg-effects.md); these types change when it does.

use super::value::V;
use serde::de::{Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

/// The whole file.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub generated: String,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub systems: BTreeMap<String, System>,
    pub filters: BTreeMap<String, Filter>,
    /// The hand-written effects, by id.
    #[serde(default)]
    pub effects: BTreeMap<String, Effect>,
    /// The words a person reads in the menus, in each language the apps can be set to (see `Catalog::localized`).
    #[serde(default)]
    pub i18n: BTreeMap<String, Translation>,
}

/// The translated words of one language.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Translation {
    #[serde(default)]
    pub categories: BTreeMap<String, Text>,
    #[serde(default)]
    pub filters: BTreeMap<String, Text>,
    #[serde(default)]
    pub effects: BTreeMap<String, Text>,
}

/// A category has a `title` and a `hint`; a filter or an effect has a name (`plain`) and a sentence (`what`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Text {
    pub title: Option<String>,
    pub hint: Option<String>,
    pub plain: Option<String>,
    pub what: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Category {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub hint: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct System {
    pub version: Option<String>,
}

/// One ffmpeg filter.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Filter {
    #[serde(default)]
    pub role: String,
    /// `video`, `audio` or `both`.
    #[serde(default)]
    pub works: String,
    #[serde(default)]
    pub plain: Option<String>,
    #[serde(default)]
    pub what: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub level: Option<String>,
    /// Why the app cannot offer it yet.
    #[serde(default)]
    pub blocked: Option<String>,
    /// The first version each system's build listed it; `null` = that system is not listed; empty = not on it.
    #[serde(default)]
    pub since: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub until: BTreeMap<String, Option<String>>,
    /// The hand-written effects that cover this filter (it has no card of its own).
    #[serde(default, rename = "in")]
    pub covered_by: Vec<String>,
    /// Which of our apps has it built in, and under what name.
    #[serde(default)]
    pub builtin: BTreeMap<String, String>,
    /// What it changes of the stream it hands on: `size` of the picture, `time` of the clip.
    #[serde(default)]
    pub changes: Vec<String>,
    /// What it must receive final: `size` (runs on the delivered picture), `time`.
    #[serde(default)]
    pub wants: Vec<String>,
    /// The option that holds how many times faster the clip plays (a filter that changes the time): what the app divides the length by.
    #[serde(default)]
    pub speed: Option<String>,
    #[serde(default)]
    pub options: Vec<FOption>,
}

/// One option of a filter.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct FOption {
    pub name: String,
    #[serde(default)]
    pub short: Vec<String>,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default, deserialize_with = "some_json")]
    pub default: Option<Value>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    /// The kind of control the type calls for.
    #[serde(default)]
    pub control: String,
    #[serde(default)]
    pub plain: Option<String>,
    #[serde(default)]
    pub what: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub step: Option<f64>,
    /// A narrower range for a slider than ffmpeg's own.
    #[serde(default)]
    pub soft: Option<Soft>,
    /// Another kind of control than the type says.
    #[serde(default, rename = "as")]
    pub as_: Option<As>,
    /// How text is written into the filter text: `path`, `quoted` or `drawtext`.
    #[serde(default)]
    pub escape: Option<String>,
    #[serde(default)]
    pub required: bool,
    /// ffmpeg stores this option in the same variable as another: it is hidden and never written.
    #[serde(default)]
    pub alias_of: Option<String>,
    #[serde(default)]
    pub choices: Option<Vec<OptChoice>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub struct Soft {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct As {
    #[serde(default)]
    pub control: Option<String>,
    #[serde(default)]
    pub choices: Option<Vec<AsChoice>>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct AsChoice {
    #[serde(default, deserialize_with = "some_json")]
    pub value: Option<Value>,
    #[serde(default)]
    pub plain: Option<String>,
}

/// A choice of an option, as ffmpeg lists it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct OptChoice {
    pub name: String,
    #[serde(default, deserialize_with = "some_json")]
    pub value: Option<Value>,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub plain: Option<String>,
    /// A reason: ffmpeg itself refuses this choice, so a form does not offer it.
    #[serde(default)]
    pub broken: Option<String>,
}

/// An effect: written by a person (`effects` of the file), or the one a filter is by itself (`implicit`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Effect {
    #[serde(default)]
    pub plain: Option<String>,
    #[serde(default)]
    pub what: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub works: String,
    #[serde(default)]
    pub builtin: BTreeMap<String, String>,
    /// What it changes of the stream it hands on: `size` of the picture, `time` of the clip.
    #[serde(default)]
    pub changes: Vec<String>,
    /// What it must receive final: `size` (runs on the delivered picture), `time`.
    #[serde(default)]
    pub wants: Vec<String>,
    /// True when the picture carries the file's subtitles: the output then holds no subtitle track.
    #[serde(default, rename = "burnsSubtitles")]
    pub burns_subtitles: bool,
    /// The filters that leave the picker because this effect replaces them.
    #[serde(default)]
    pub covers: Vec<String>,
    /// An effect whose work the app does itself, from what it measures of the file: which operation, and which control feeds each of its inputs.
    #[serde(default)]
    pub measure: Option<Measure>,
    #[serde(default)]
    pub controls: Vec<ControlDef>,
    #[serde(default)]
    pub steps: Option<Vec<Step>>,
    #[serde(default)]
    pub graph: Option<Vec<Chain>>,
    #[serde(default)]
    pub audio: Option<Vec<Step>>,
    /// Extra input files, each named by a control.
    #[serde(default)]
    pub inputs: Vec<InputRef>,
    /// Formulas over controls, variables and context values.
    #[serde(default)]
    pub derive: BTreeMap<String, String>,
    #[serde(default)]
    pub since: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub until: BTreeMap<String, Option<String>>,
    /// True for the effect a filter is by itself: only a value that differs from the default is written.
    #[serde(skip)]
    pub implicit: bool,
    /// The filter's name, for an implicit effect.
    #[serde(skip)]
    pub name: String,
}

/// What the app does for a measured effect: `op` names the operation (the app knows a fixed list), `controls` says which control of the
/// effect holds each input of that operation.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Measure {
    pub op: String,
    #[serde(default)]
    pub controls: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct InputRef {
    #[serde(rename = "$")]
    pub control: String,
}

/// A control of an effect: bound to a filter option (`bind`: "filter.option"), or its own.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ControlDef {
    pub id: String,
    #[serde(default)]
    pub bind: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default)]
    pub control: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default, deserialize_with = "some_json")]
    pub default: Option<Value>,
    #[serde(default)]
    pub choices: Option<Vec<ChoiceDef>>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub ext: Option<String>,
    #[serde(default)]
    pub example: Option<String>,
}

/// A choice of an own control.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ChoiceDef {
    #[serde(default, deserialize_with = "some_json")]
    pub value: Option<Value>,
    #[serde(default)]
    pub label: Option<String>,
    /// Variables this choice sets.
    #[serde(default)]
    pub set: Option<BTreeMap<String, String>>,
    /// The filters this choice stands for.
    #[serde(default)]
    pub steps: Option<Vec<Step>>,
}

/// One step of a chain: a filter with the values of its options, or `{ choice }` = the steps of the chosen choice of a control.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Step {
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub choice: Option<String>,
    /// In the order the file gives them: it is the order they are written in.
    #[serde(default, rename = "with")]
    pub with: Ordered<With>,
}

/// A piece of a filter graph: from the labelled inputs, through the steps, to the labelled outputs.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Chain {
    #[serde(default)]
    pub from: Vec<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub to: Vec<String>,
}

/// The value of an option in a step.
#[derive(Debug, Clone, PartialEq)]
pub enum With {
    Literal(V),
    /// `{ "$": controlId }`
    Control(String),
    /// `{ "expr": "text with {variables}" }`
    Expr(String),
    /// `{ "ctx": "input" | "font" }`
    Ctx(String),
    /// An object of a form this version does not know.
    Unknown,
}

impl<'de> Deserialize<'de> for With {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<With, D::Error> {
        let v = Value::deserialize(d)?;
        Ok(match &v {
            Value::Object(o) => {
                let text = |k: &str| o.get(k).map(|x| V::from_json(x).js_string());
                if let Some(c) = text("$") {
                    With::Control(c)
                } else if let Some(e) = text("expr") {
                    With::Expr(e)
                } else if let Some(c) = text("ctx") {
                    With::Ctx(c)
                } else {
                    With::Unknown
                }
            }
            other => With::Literal(V::from_json(other)),
        })
    }
}

/// A JSON object whose keys are kept in the order of the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Ordered<T>(pub Vec<(String, T)>);

impl<T> Default for Ordered<T> {
    fn default() -> Self {
        Ordered(Vec::new())
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Ordered<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Walk<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Walk<T> {
            type Value = Ordered<T>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, T>()? {
                    out.push((k, v));
                }
                Ok(Ordered(out))
            }
        }
        d.deserialize_map(Walk(std::marker::PhantomData))
    }
}

/// `Option<Value>` that keeps a JSON `null` as `Some(Null)`: "absent" and "null" are two things in the JavaScript engine.
fn some_json<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

impl Catalog {
    pub fn from_json(text: &str) -> Result<Catalog, String> {
        serde_json::from_str(text).map_err(|e| format!("effects.json: {e}"))
    }

    /// The catalogue with the words of `lang` (`fr`, `pt_BR`...) in place of the English ones; a word with no translation stays English.
    /// The copy carries no translations: it is the one an app works with.
    pub fn localized(&self, lang: &str) -> Catalog {
        let mut c = Catalog {
            generated: self.generated.clone(),
            categories: self.categories.clone(),
            systems: self.systems.clone(),
            filters: self.filters.clone(),
            effects: self.effects.clone(),
            i18n: BTreeMap::new(),
        };
        let Some(t) = self.i18n.get(lang) else { return c };
        for cat in &mut c.categories {
            if let Some(x) = t.categories.get(&cat.id) {
                if let Some(v) = &x.title {
                    cat.title = v.clone();
                }
                if let Some(v) = &x.hint {
                    cat.hint = v.clone();
                }
            }
        }
        for (name, f) in &mut c.filters {
            if let Some(x) = t.filters.get(name) {
                f.plain = x.plain.clone().or(f.plain.take());
                f.what = x.what.clone().or(f.what.take());
            }
        }
        for (id, e) in &mut c.effects {
            if let Some(x) = t.effects.get(id) {
                e.plain = x.plain.clone().or(e.plain.take());
                e.what = x.what.clone().or(e.what.take());
            }
        }
        c
    }
}
