//! NumberField — twin of `ui-ctl-numberfield.jsx`: a numeric input with themed ▲▼ steppers instead
//! of the browser's own spinner. Same classes and element order (`.numfield`, `.numfield-input`,
//! `.numfield-steppers`, `.numfield-step`), so `ui-app.css` skins it unchanged.
//!
//! Behaviour is the JSX's: the field keeps its own draft so the user can type freely (including a
//! value below `min` on the way to a larger one), it emits a valid number while typing, and it
//! CLAMPS to `[min, max]` on blur and on the steppers. `allow_empty` lets it be blank, which emits
//! `None` - for "automatic" fields.

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
    value: Option<u32>,
    #[props(default)] min: Option<u32>,
    #[props(default)] max: Option<u32>,
    #[props(default = 1)] step: u32,
    /// Width in px. A number is a few digits wide, so a caller should always give one.
    #[props(default)] width: Option<u32>,
    #[props(default)] placeholder: String,
    #[props(default = false)] disabled: bool,
    #[props(default = false)] allow_empty: bool,
    #[props(default)] title: String,
    on_change: EventHandler<Option<u32>>,
) -> Element {
    let mut draft = use_signal(|| value.map(|v| v.to_string()).unwrap_or_default());
    // Mirror the controlled value into the draft; a keystroke that was in range already pushed the
    // same value up, so this only differs after a clamp or an outside change.
    use_effect(use_reactive!(|value| draft.set(value.map(|v| v.to_string()).unwrap_or_default())));

    let clamp = move |v: u32| v.max(min.unwrap_or(0)).min(max.unwrap_or(u32::MAX));
    let bump = move |dir: i64| {
        if disabled {
            return;
        }
        let next = (i64::from(value.unwrap_or(min.unwrap_or(0))) + dir * i64::from(step)).max(0) as u32;
        on_change.call(Some(clamp(next)));
    };

    rsx! {
        div {
            class: "numfield",
            style: if let Some(w) = width { format!("width: {w}px") } else { String::new() },
            title: "{title}",
            input {
                class: "numfield-input",
                r#type: "number",
                value: "{draft}",
                min: min.map(|v| v.to_string()),
                max: max.map(|v| v.to_string()),
                step: "{step}",
                placeholder: "{placeholder}",
                disabled,
                oninput: move |e| {
                    let raw = e.value();
                    draft.set(raw.clone());
                    if allow_empty && raw.trim().is_empty() {
                        on_change.call(None);
                    } else if let Ok(n) = raw.trim().parse::<u32>() {
                        if n >= min.unwrap_or(0) && n <= max.unwrap_or(u32::MAX) {
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
                    let n = raw.trim().parse::<u32>().unwrap_or_else(|_| min.unwrap_or(0));
                    let n = clamp(n);
                    draft.set(n.to_string());
                    on_change.call(Some(n));
                },
            }
            span { class: "numfield-steppers", "aria-hidden": "true",
                button {
                    class: "numfield-step",
                    r#type: "button",
                    tabindex: "-1",
                    disabled,
                    onclick: move |_| bump(1),
                    Chevron { up: true }
                }
                button {
                    class: "numfield-step",
                    r#type: "button",
                    tabindex: "-1",
                    disabled,
                    onclick: move |_| bump(-1),
                    Chevron { up: false }
                }
            }
        }
    }
}
