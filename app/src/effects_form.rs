//! EffectForm — the form of one effect: a field per control, as `effects.json` describes them. Rsx twin of the form half of `effects-form.js`
//! (the engine and the menu are `engine::ffmpeg_effects`).
//!
//! ⚠ SOURCE OF TRUTH: the shared ffmpeg-effects folder (`rust/app/effects_form.rs`). A copy in an app is never edited there.
//!
//! Controlled: the form shows `values` (the control id → value map the engine reads) and calls `on_change` with the whole map
//! after a field changed; the caller keeps it, builds the ffmpeg text from it (`engine::ffmpeg_effects::args`) and stores it.
//! Like `ColorPicker` it carries no i18n coupling: every word it writes comes in `FormText`, and the lists of names ffmpeg knows
//! (sizes, frame rates, pixel formats ...) come in `Named`, read from `ffmpeg-named-values.json` (empty = the fields work, offer no names).
//! A file field has a Browse button that only reports (`on_browse`): the caller opens its dialog and puts the path back in `values`.
//!
//! Needs from the app's catalog: `ColorPicker`, `NumberInput` (= the shared NumberField). The class names are the app's own
//! (`dlg-field`, `dlg-field-label`, `input`, `select`, `btn`, `hint`); the few of its own are in `effects_form.css`, beside this file.
//! Left out on purpose (the catalogue pages show them, a user does not need them): the check results under a form, the origin tooltip
//! of a label, the raw ffmpeg name beside it, the "Not listed ... second spellings" note and the command box.

use crate::catalog::{ColorPicker, NumberInput};
use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdRotateCcw};
use engine::ffmpeg_effects::{
    Catalog, Choice, Control, Ctx, Dur, Effect, Filters, Ordered, V, Values, args, defaults, dur_seconds, flag_names, flag_set, flag_text, js_number_string, resolve_controls,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::sync::Arc;

/// The catalogue, shared by every form on the page; two refs are equal when they are the same catalogue.
#[derive(Clone)]
pub struct CatalogRef(pub Arc<Catalog>);

impl PartialEq for CatalogRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// The lists of names ffmpeg knows, as `ffmpeg-named-values.json` holds them.
#[derive(Debug, Default, Deserialize)]
pub struct NamedData {
    #[serde(default)]
    pub sizes: Ordered<String>,
    #[serde(default)]
    pub rates: Ordered<String>,
    #[serde(default)]
    pub pixel: Vec<String>,
    #[serde(default)]
    pub sample: Vec<String>,
    #[serde(default)]
    pub layouts: Vec<String>,
}

#[derive(Clone, Default)]
pub struct Named(pub Arc<NamedData>);

impl PartialEq for Named {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Named {
    pub fn from_json(text: &str) -> Result<Named, String> {
        serde_json::from_str::<NamedData>(text).map(|d| Named(Arc::new(d))).map_err(|e| e.to_string())
    }

    fn size(&self, name: &str) -> Option<&str> {
        self.0.sizes.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    fn rate(&self, name: &str) -> Option<&str> {
        self.0.rates.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// Which list a text control takes its values from.
    fn list(&self, id: &str) -> &[String] {
        if matches!(id, "pix_fmt" | "pixel_formats" | "format") {
            &self.0.pixel
        } else if id.contains("sample_fmt") {
            &self.0.sample
        } else if id.contains("channel_layout") || id.starts_with("chl_") {
            &self.0.layouts
        } else {
            &[]
        }
    }
}

/// Every word the form writes.
#[derive(Clone, Default, PartialEq)]
pub struct FormText {
    /// "Nothing to set."
    pub nothing: String,
    /// The Browse button of a file field.
    pub browse: String,
    /// The first line of the named-size drop-down ("named ...").
    pub named_size: String,
    /// The colour button's tooltip, and the picker's two buttons.
    pub pick_colour: String,
    pub cancel: String,
    pub apply: String,
    /// What a frame-rate / time field says when it cannot read what was typed.
    pub not_a_rate: String,
    pub not_a_time: String,
    pub placeholder_rate: String,
    pub placeholder_time: String,
    pub placeholder_colour: String,
    /// A file / folder / save-file field when empty.
    pub placeholder_file: String,
    pub placeholder_folder: String,
    pub placeholder_save: String,
    /// The reset button's tooltip ("Back to the default").
    pub reset: String,
}

/// A Browse button was pressed: which control (its id) and what it browses (`file`, `saveFile` or `folder`).
#[derive(Clone, Debug, PartialEq)]
pub struct BrowseRequest {
    pub id: String,
    pub kind: String,
}

fn value_text(v: Option<&V>) -> String {
    match v {
        None | Some(V::Null) => String::new(),
        Some(v) => v.js_string(),
    }
}

fn number_of(v: Option<&V>) -> Option<f64> {
    match v {
        Some(V::Num(n)) => Some(*n),
        Some(V::Str(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn is_on(v: Option<&V>) -> bool {
    match v {
        Some(V::Bool(b)) => *b,
        Some(V::Num(n)) => *n == 1.0,
        Some(V::Str(s)) => s == "true",
        _ => false,
    }
}

/// A size is two numbers written `WxH`; ffmpeg also knows names (`hd720`), the list gives their numbers.
fn size_parts(named: &Named, s: &str) -> Option<(String, String)> {
    let s = s.trim();
    if s.is_empty() {
        return Some((String::new(), String::new()));
    }
    let split = |t: &str| {
        let (w, h) = t.split_once('x')?;
        let digits = |x: &str| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit());
        (digits(w) && digits(h)).then(|| (w.to_string(), h.to_string()))
    };
    split(s).or_else(|| named.size(s).and_then(split))
}

/// A fraction (a frame rate, a pixel shape): `n/d`, a decimal, or a name. `None` = empty, `Some(NaN)` = not readable.
fn frac_value(named: &Named, s: &str) -> Option<f64> {
    let mut s = s.trim().to_string();
    if s.is_empty() {
        return None;
    }
    if let Some(r) = named.rate(&s) {
        s = r.to_string();
    }
    if let Some((n, d)) = s.split_once('/') {
        return match (n.parse::<i64>(), d.parse::<u64>()) {
            (Ok(n), Ok(d)) if d != 0 => Some(n as f64 / d as f64),
            _ => Some(f64::NAN),
        };
    }
    Some(s.parse::<f64>().ok().filter(|n| n.is_finite() && !s.contains(['e', 'E', 'i', 'n', 'N', '+'])).unwrap_or(f64::NAN))
}

fn round_to(x: f64, places: f64) -> f64 {
    (x * places).round() / places
}

/// A colour's `#rrggbb`: what the picker starts from. A name or anything else starts it at black.
fn colour_hex(s: &str) -> String {
    let s = s.trim();
    let body = s.strip_prefix('#').or_else(|| s.strip_prefix("0x")).or_else(|| s.strip_prefix("0X"));
    match body {
        Some(b) if b.len() >= 6 && b[..6].chars().all(|c| c.is_ascii_hexdigit()) => format!("#{}", b[..6].to_ascii_lowercase()),
        _ => "#000000".to_string(),
    }
}

#[component]
pub fn EffectForm(
    catalog: CatalogRef,
    /// The form's key: a hand-written effect's id, or `try:<filter>`.
    effect: String,
    values: Values,
    on_change: EventHandler<Values>,
    #[props(default)] named: Named,
    #[props(default)] text: FormText,
    #[props(default)] on_browse: EventHandler<BrowseRequest>,
) -> Element {
    let Some(form) = catalog.0.form(&effect) else {
        return rsx! {};
    };
    let controls = resolve_controls(&form, &catalog.0.filters).unwrap_or_default();
    let command = command_text(&form, &catalog.0.filters, &values);
    let starts = defaults(&form, &catalog.0.filters).unwrap_or_default();
    if controls.is_empty() {
        return rsx! {
            p { class: "hint", "{text.nothing}" }
            pre { class: "cmd", "{command}" }
        };
    }
    rsx! {
        div { class: "effect-form",
            for c in controls {
                Field {
                    key: "{c.id}",
                    start: starts.get(&c.id).cloned(),
                    control: c,
                    values: values.clone(),
                    on_change,
                    named: named.clone(),
                    text: text.clone(),
                    on_browse,
                }
            }
            pre { class: "cmd", "{command}" }
        }
    }
}

/// What the form's choices turn into for ffmpeg, as the pages show it (one `-flag value` per line; a value with a space or a
/// filter-graph bracket in double quotes). An input and a font stand in for the ones the conversion will name.
fn command_text(form: &Effect, filters: &Filters, values: &Values) -> String {
    let ctx: Ctx = [("input", "C:/Videos/movie.mkv"), ("font", "C:/Windows/Fonts/arial.ttf"), ("duration", "180")]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    match args(form, filters, values, &ctx) {
        Ok(a) => a
            .chunks(2)
            .map(|p| match p {
                [flag, v] if v.contains(|c: char| c.is_whitespace() || ";[]'".contains(c)) => format!("{flag} \"{v}\""),
                [flag, v] => format!("{flag} {v}"),
                [flag] => flag.clone(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("
"),
        Err(e) => format!("cannot build: {e}"),
    }
}

#[component]
fn Field(control: Control, start: Option<V>, values: Values, on_change: EventHandler<Values>, named: Named, text: FormText, on_browse: EventHandler<BrowseRequest>) -> Element {
    let c = control;
    let id = c.id.clone();
    // an EventHandler is Copy, so every field closure below can hold it
    let set = EventHandler::new({
        let values = values.clone();
        let id = id.clone();
        move |v: V| {
            let mut next = values.clone();
            next.insert(id.clone(), v);
            on_change.call(next);
        }
    });
    let reset = EventHandler::new({
        let values = values.clone();
        let id = id.clone();
        let start = start.clone();
        move |_: ()| {
            let mut next = values.clone();
            match &start {
                Some(v) => next.insert(id.clone(), v.clone()),
                None => next.remove(&id),
            };
            on_change.call(next);
        }
    });
    let current = values.get(&c.id);
    let at_start = value_text(current) == value_text(start.as_ref());
    let shown = value_text(current);
    let star = if c.required { " *" } else { "" };
    let label = format!("{}{star}", c.label);
    let help = c.help.clone().filter(|h| !h.is_empty());
    rsx! {
        div { class: "dlg-field",
            label { class: "dlg-field-label", "{label}" }
            div { class: "effect-ctl",
                match c.control.as_str() {
                    "list" => rsx! { ListField { choices: c.choices.clone(), value: shown.clone(), on_pick: set } },
                    "switch" => rsx! {
                        input {
                            r#type: "checkbox",
                            checked: is_on(current),
                            onchange: move |e| set.call(V::Bool(e.checked())),
                        }
                    },
                    "integer" | "decimal" => rsx! { NumberRow { control: c.clone(), value: number_of(current), on_pick: set } },
                    "size" => rsx! { SizeRow { value: shown.clone(), named: named.clone(), text: text.clone(), on_pick: set } },
                    "fraction" => {
                        let read = match frac_value(&named, &shown) {
                            None => String::new(),
                            Some(x) if x.is_nan() => text.not_a_rate.clone(),
                            Some(x) => format!("= {}", js_number_string(round_to(x, 1e4))),
                        };
                        let bad = frac_value(&named, &shown).is_some_and(f64::is_nan);
                        let dl = format!("dl-{}", c.id);
                        rsx! {
                            input {
                                class: "input",
                                r#type: "text",
                                list: "{dl}",
                                value: "{shown}",
                                placeholder: "{text.placeholder_rate}",
                                oninput: move |e| set.call(V::Str(e.value())),
                            }
                            datalist { id: "{dl}",
                                for (k , _) in named.0.rates.0.iter() {
                                    option { value: "{k}" }
                                }
                                for k in ["24", "25", "30", "50", "60", "24000/1001", "30000/1001", "60000/1001"] {
                                    option { value: "{k}" }
                                }
                            }
                            span { class: if bad { "unit bad" } else { "unit" }, "{read}" }
                        }
                    }
                    "format" => rsx! { ListedText { id: c.id.clone(), value: shown.clone(), named: named.clone(), on_pick: set } },
                    "file" | "saveFile" | "folder" => {
                        let kind = c.control.clone();
                        let hold = match kind.as_str() {
                            "folder" => text.placeholder_folder.clone(),
                            "saveFile" => text.placeholder_save.clone(),
                            _ => text.placeholder_file.clone(),
                        };
                        rsx! {
                            input {
                                class: "input",
                                r#type: "text",
                                value: "{shown}",
                                placeholder: "{hold}",
                                oninput: move |e| set.call(V::Str(e.value())),
                            }
                            button {
                                class: "btn",
                                r#type: "button",
                                onclick: move |_| on_browse.call(BrowseRequest { id: id.clone(), kind: kind.clone() }),
                                "{text.browse}"
                            }
                        }
                    }
                    "checkboxes" if c.choices.iter().any(|x| !matches!(x.value.js_string().as_str(), "all" | "none")) => rsx! {
                        FlagBoxes { choices: c.choices.clone(), value: shown.clone(), on_pick: set }
                    },
                    "duration" => {
                        let (read, bad) = match dur_seconds(&shown) {
                            Dur::Empty => (String::new(), false),
                            Dur::Bad => (text.not_a_time.clone(), true),
                            Dur::Seconds(s) => (format!("= {} s", js_number_string(round_to(s, 1e6))), false),
                        };
                        rsx! {
                            input {
                                class: "input",
                                r#type: "text",
                                inputmode: "decimal",
                                value: "{shown}",
                                placeholder: "{text.placeholder_time}",
                                oninput: move |e| set.call(V::Str(e.value())),
                            }
                            span { class: if bad { "unit bad" } else { "unit" }, "{read}" }
                        }
                    }
                    "colour" => rsx! {
                        ColorPicker {
                            color: colour_hex(&shown),
                            on_change: move |hex: String| set.call(V::Str(hex)),
                            cancel_label: text.cancel.clone(),
                            apply_label: text.apply.clone(),
                            pick_title: text.pick_colour.clone(),
                        }
                        input {
                            class: "input",
                            r#type: "text",
                            value: "{shown}",
                            placeholder: "{text.placeholder_colour}",
                            oninput: move |e| set.call(V::Str(e.value())),
                        }
                    },
                    "text" if c.soft.is_some() => rsx! { ExpressionRow { control: c.clone(), value: shown.clone(), on_pick: set } },
                    _ => rsx! {
                        input { class: "input", r#type: "text", value: "{shown}", oninput: move |e| set.call(V::Str(e.value())) }
                    },
                }
                button {
                    class: "btn icon fx-reset",
                    r#type: "button",
                    title: "{text.reset}",
                    disabled: at_start,
                    onclick: move |_| reset.call(()),
                    Icon { width: 12, height: 12, icon: LdRotateCcw }
                }
            }
            if let Some(h) = help {
                p { class: "hint", "{h}" }
            }
        }
    }
}

#[component]
fn ListField(choices: Vec<Choice>, value: String, on_pick: EventHandler<V>) -> Element {
    rsx! {
        select {
            class: "select",
            value: "{value}",
            onchange: {
                let choices = choices.clone();
                move |e: Event<FormData>| {
                    let picked = e.value();
                    if let Some(c) = choices.iter().find(|c| c.value.js_string() == picked) {
                        on_pick.call(c.value.clone());
                    }
                }
            },
            for c in choices.iter() {
                option { value: "{c.value.js_string()}", selected: c.value.js_string() == value, "{c.label}" }
            }
        }
    }
}

#[component]
fn NumberRow(control: Control, value: Option<f64>, on_pick: EventHandler<V>) -> Element {
    let c = control;
    let integer = c.control == "integer";
    let step = c.step.unwrap_or(if integer { 1.0 } else { 0.05 });
    let (lo, hi) = match c.soft {
        Some(s) => (Some(s.min), Some(s.max)),
        None => (c.min, c.max),
    };
    // a slider only where the range is narrow enough to be usable
    let slider = match (lo, hi) {
        (Some(lo), Some(hi)) if hi - lo <= 500.0 => Some((lo, hi)),
        _ => None,
    };
    // the places the box shows: as many as the finest number the option can hold (its step, its default, the ends of its suggested range),
    // so a value that can be 0.05 reads 0.75 and 0.6 reads 0.60; a whole-number control shows none
    let places = |x: f64| {
        let t = x.to_string();
        if t.contains('e') { 0 } else { t.split_once('.').map_or(0, |(_, d)| d.len()) }
    };
    let decimals = if integer {
        None
    } else {
        let mut n = places(step);
        if let Some(Some(d)) = c.default.as_ref().map(|d| match d { V::Num(x) => Some(*x), V::Str(s) => s.trim().parse::<f64>().ok(), _ => None }) {
            n = n.max(places(d));
        }
        if let Some(sf) = c.soft {
            n = n.max(places(sf.min)).max(places(sf.max));
        }
        Some(n.min(6))
    };
    rsx! {
        if let Some((lo , hi)) = slider {
            input {
                r#type: "range",
                min: "{lo}",
                max: "{hi}",
                step: "{step}",
                value: "{value.unwrap_or(lo)}",
                oninput: move |e| {
                    if let Ok(n) = e.value().parse::<f64>() {
                        on_pick.call(V::Num(n));
                    }
                },
            }
        }
        NumberInput {
            value,
            min: c.min,
            max: c.max,
            step,
            decimals,
            allow_empty: true,
            on_change: move |n: Option<f64>| on_pick.call(n.map(V::Num).unwrap_or_else(|| V::Str(String::new()))),
        }
        if let Some(u) = c.unit.clone() {
            span { class: "unit", "{u}" }
        }
    }
}

/// A number or an expression: a slider over the suggested range beside the text box. The slider writes a plain number; what is typed
/// in the box (an expression included) is kept as typed, and the slider shows the box's value when it is a number.
#[component]
fn ExpressionRow(control: Control, value: String, on_pick: EventHandler<V>) -> Element {
    let Some(soft) = control.soft else { return rsx! {} };
    let step = control.step.unwrap_or(0.05);
    let at = value.trim().parse::<f64>().unwrap_or(soft.min);
    rsx! {
        input {
            r#type: "range",
            min: "{soft.min}",
            max: "{soft.max}",
            step: "{step}",
            value: "{at}",
            oninput: move |e| {
                if let Ok(n) = e.value().parse::<f64>() {
                    on_pick.call(V::Num(n));
                }
            },
        }
        input { class: "input", r#type: "text", value: "{value}", oninput: move |e| on_pick.call(V::Str(e.value())) }
    }
}

#[component]
fn SizeRow(value: String, named: Named, text: FormText, on_pick: EventHandler<V>) -> Element {
    // a size ffmpeg writes some other way is shown as plain text, as the pages do
    let Some((w0, h0)) = size_parts(&named, &value) else {
        return rsx! {
            input { class: "input", r#type: "text", value: "{value}", oninput: move |e| on_pick.call(V::Str(e.value())) }
        };
    };
    let mut w = use_signal(|| w0.clone());
    let mut h = use_signal(|| h0.clone());
    // an outside change (a named pick, a reset) replaces what was typed
    use_effect(use_reactive!(|w0, h0| {
        w.set(w0.clone());
        h.set(h0.clone());
    }));
    let emit = move |w: String, h: String| on_pick.call(V::Str(if !w.is_empty() && !h.is_empty() { format!("{w}x{h}") } else { String::new() }));
    rsx! {
        input {
            class: "input sz",
            r#type: "number",
            min: "0",
            value: "{w}",
            oninput: move |e| {
                w.set(e.value());
                emit(e.value(), h());
            },
        }
        span { class: "unit", "×" }
        input {
            class: "input sz",
            r#type: "number",
            min: "0",
            value: "{h}",
            oninput: move |e| {
                h.set(e.value());
                emit(w(), e.value());
            },
        }
        select {
            class: "select szp",
            value: "",
            onchange: move |e| {
                let v = e.value();
                if !v.is_empty() {
                    on_pick.call(V::Str(v));
                }
            },
            option { value: "", "{text.named_size}" }
            for (k , x) in named.0.sizes.0.iter() {
                option { value: "{x}", "{k} ({x})" }
            }
        }
    }
}

#[component]
fn ListedText(id: String, value: String, named: Named, on_pick: EventHandler<V>) -> Element {
    let dl = format!("dl-{id}");
    rsx! {
        input {
            class: "input",
            r#type: "text",
            list: "{dl}",
            value: "{value}",
            oninput: move |e| on_pick.call(V::Str(e.value())),
        }
        datalist { id: "{dl}",
            for k in named.list(&id).iter() {
                option { value: "{k}" }
            }
        }
    }
}

#[component]
fn FlagBoxes(choices: Vec<Choice>, value: String, on_pick: EventHandler<V>) -> Element {
    let on: BTreeSet<String> = flag_set(&choices, &value).unwrap_or_default();
    let names = flag_names(&choices);
    rsx! {
        div { class: "flags",
            for c in choices.iter().filter(|c| names.contains(&c.value.js_string())) {
                {
                    let name = c.value.js_string();
                    let checked = on.contains(&name);
                    let (on, choices) = (on.clone(), choices.clone());
                    rsx! {
                        label { key: "{name}",
                            input {
                                r#type: "checkbox",
                                checked,
                                onchange: move |e| {
                                    let mut next = on.clone();
                                    if e.checked() {
                                        next.insert(name.clone());
                                    } else {
                                        next.remove(&name);
                                    }
                                    on_pick.call(V::Str(flag_text(&choices, &next)));
                                },
                            }
                            " {c.label}"
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named() -> Named {
        Named::from_json(r#"{"sizes":{"hd720":"1280x720"},"rates":{"ntsc":"30000/1001"},"pixel":["yuv420p"]}"#).unwrap()
    }

    #[test]
    fn a_size_is_two_numbers_or_a_name() {
        let n = named();
        assert_eq!(size_parts(&n, ""), Some((String::new(), String::new())));
        assert_eq!(size_parts(&n, "640x480"), Some(("640".into(), "480".into())));
        assert_eq!(size_parts(&n, "HD720"), Some(("1280".into(), "720".into())));
        assert_eq!(size_parts(&n, "iw/2xih"), None);
    }

    #[test]
    fn a_fraction_is_read_as_the_page_reads_it() {
        let n = named();
        assert_eq!(frac_value(&n, ""), None);
        assert_eq!(frac_value(&n, "25"), Some(25.0));
        assert_eq!(frac_value(&n, "30000/1001"), Some(30000.0 / 1001.0));
        assert_eq!(frac_value(&n, "ntsc"), Some(30000.0 / 1001.0));
        assert_eq!(frac_value(&n, "29.97"), Some(29.97));
        assert!(frac_value(&n, "fast").unwrap().is_nan());
        assert!(frac_value(&n, "1/0").unwrap().is_nan());
        assert_eq!(js_number_string(round_to(30000.0 / 1001.0, 1e4)), "29.97");
    }

    #[test]
    fn a_colour_starts_the_picker_at_its_hex() {
        assert_eq!(colour_hex("#FF8800"), "#ff8800");
        assert_eq!(colour_hex("0x00ff00@0.5"), "#00ff00");
        assert_eq!(colour_hex("red"), "#000000");
    }

    #[test]
    fn the_named_lists_are_picked_by_the_option() {
        let n = named();
        assert_eq!(n.list("pix_fmt"), ["yuv420p".to_string()]);
        assert!(n.list("radius").is_empty());
    }
}
