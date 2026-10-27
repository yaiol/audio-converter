// twin: ui-ctl-numberfield.jsx jsx@31ac7b831757 rsx@cbb433364151
//! NumberField — twin of `ui-ctl-numberfield.jsx`: a numeric input with themed ▲▼ steppers instead
//! of the browser's own spinner. Same classes and element order (`.numfield`, `.numfield-input`,
//! `.numfield-steppers`, `.numfield-step`), so `ui-app.css` skins it unchanged.
//!
//! Behaviour is the JSX's: the field keeps its own draft so the user can type freely (including a value below
//! `min` on the way to a larger one), it emits a valid number while typing, and it CLAMPS to `[min, max]` on blur
//! and on the steppers. Any real number: decimals and negatives (the value is `f64`). `allow_empty` lets it be
//! blank, which emits `None` - for "automatic" fields (the JSX emits `''`).
//! `width` is a CSS length (`"56px"`, `"100%"`), as the JSX's string form; `class` is its `className`.
//! Differs: behaviour none. A caller's extra attributes are spread onto the input as the JSX's `...rest` is (event
//! handlers go through the same spread); the value is `Option<f64>` where the JSX's is a number or `''`.

use dioxus::prelude::*;

#[component]
fn Chevron(up: bool) -> Element {
    let points = if up { "6 15 12 9 18 15" } else { "6 9 12 15 18 9" };
    rsx! {
        svg {
            width: "9",
            height: "9",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "3",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            polyline { points }
        }
    }
}

#[component]
pub fn NumberField(
    value: Option<f64>,
    #[props(default)] min: Option<f64>,
    #[props(default)] max: Option<f64>,
    #[props(default = 1.0)] step: f64,
    /// How many places the field shows: a number is written with exactly that many (0.6 as 0.60) once it is not being typed.
    #[props(default)] decimals: Option<usize>,
    /// A CSS length: `"56px"`, `"100%"`. Empty = the stylesheet's own width.
    #[props(default)] width: String,
    #[props(default = false)] disabled: bool,
    #[props(default = false)] allow_empty: bool,
    #[props(default)] class: String,
    #[props(default)] title: String,
    on_change: EventHandler<Option<f64>>,
    /// Any attribute of an `<input>` (`placeholder`, `aria-label`, `name` ...), put on the field itself - the JSX's `...rest`.
    #[props(extends = GlobalAttributes, extends = input)]
    attributes: Vec<Attribute>,
) -> Element {
    let show = move |v: Option<f64>| v.map(|n| match decimals { Some(d) => format!("{n:.d$}"), None => n.to_string() }).unwrap_or_default();
    let mut draft = use_signal(|| show(value));
    // Mirror the controlled value into the draft; a keystroke that was in range already pushed the same value up,
    // so this only differs after a clamp or an outside change.
    // A keystroke already pushed its number up, so the draft is left as typed (not reformatted under the cursor) while it still reads as that number.
    use_effect(use_reactive!(|value| {
        let typed = draft.peek().trim().parse::<f64>().ok();
        if typed.is_none() || typed != value {
            draft.set(show(value));
        }
    }));

    // A number into [min, max]; NaN is blank when blank is allowed, else the minimum (or 0).
    let clamp = move |v: f64| -> Option<f64> {
        if v.is_nan() {
            return if allow_empty { None } else { Some(min.unwrap_or(0.0)) };
        }
        let v = min.map_or(v, |m| v.max(m));
        Some(max.map_or(v, |m| v.min(m)))
    };
    let bump = move |dir: f64| {
        if disabled {
            return;
        }
        // a step is added in binary floating point (0.7 + 0.05 is 0.7499999999999999): round to the decimals the step and the value carry
        let places = |x: f64| {
            let t = x.to_string();
            if t.contains('e') { 0 } else { t.split_once('.').map_or(0, |(_, d)| d.len()) }
        };
        let from = value.unwrap_or(0.0);
        let p = places(step).max(places(from)).min(12) as i32;
        let scale = 10f64.powi(p);
        on_change.call(clamp(((from + dir * step) * scale).round() / scale));
    };

    let class = if class.is_empty() { "numfield".to_string() } else { format!("numfield {class}") };
    let style = if width.is_empty() { None } else { Some(format!("width: {width}")) };

    rsx! {
        div { class: "{class}", style, title: "{title}",
            input {
                class: "numfield-input",
                r#type: "number",
                value: "{draft}",
                min: min.map(|v| v.to_string()),
                max: max.map(|v| v.to_string()),
                step: "{step}",
                disabled,
                oninput: move |e| {
                    let raw = e.value();
                    draft.set(raw.clone());
                    if allow_empty && raw.trim().is_empty() {
                        on_change.call(None);
                    } else if let Ok(n) = raw.trim().parse::<f64>() {
                        if min.is_none_or(|m| n >= m) && max.is_none_or(|m| n <= m) {
                            on_change.call(Some(n));
                        }
                    }
                },
                onblur: move |_| {
                    let raw = draft();
                    if allow_empty && raw.trim().is_empty() {
                        on_change.call(None);
                        return;
                    }
                    let n = clamp(raw.trim().parse::<f64>().unwrap_or(f64::NAN));
                    draft.set(show(n));
                    on_change.call(n);
                },
                ..attributes,
            }
            span { class: "numfield-steppers", "aria-hidden": "true",
                button {
                    class: "numfield-step",
                    r#type: "button",
                    tabindex: "-1",
                    disabled,
                    onclick: move |_| bump(1.0),
                    Chevron { up: true }
                }
                button {
                    class: "numfield-step",
                    r#type: "button",
                    tabindex: "-1",
                    disabled,
                    onclick: move |_| bump(-1.0),
                    Chevron { up: false }
                }
            }
        }
    }
}
