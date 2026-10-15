//! Knob — twin of `ui-ctl-knob.jsx`: label on top, round drag body, editable value underneath.
//! Same classes and element order, so `ui-ctl-knob.css` skins it unchanged. Behaviour is the
//! JSX's: vertical drag (~150 px spans min..max), value line commits on Enter/blur, Escape reverts.
//!
//! Pointer capture and select-all-on-focus have no Rust API, so they go through `document::eval`
//! by element id — the one place this control touches the JS bridge.

use dioxus::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

#[component]
pub fn Knob(
    label: String,
    value: f64,
    #[props(default = 0.0)] min: f64,
    #[props(default = 100.0)] max: f64,
    #[props(default = 1.0)] step: f64,
    #[props(default)] unit: String,
    #[props(default = false)] disabled: bool,
    #[props(default = false)] inline: bool,
    #[props(default = false)] arc: bool,
    #[props(default)] title: String,
    on_change: EventHandler<f64>,
) -> Element {
    let id = use_hook(|| format!("knob-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed)));
    let mut editing = use_signal(|| Option::<String>::None);
    let mut drag = use_signal(|| Option::<(f64, f64)>::None); // (start y, start value) while held

    // Quantise to `step`, clamp to range, and shed float noise (0.30000000000000004).
    let clamp_q = move |v: f64| -> f64 {
        let q = ((v - min) / step).round() * step + min;
        let fixed = (q * 1e6).round() / 1e6;
        fixed.clamp(min, max)
    };

    let frac = if max > min { (value - min) / (max - min) } else { 0.0 };
    let angle = -135.0 + 270.0 * frac; // −135°..+135°, the hardware-knob convention

    let shown = match editing() {
        Some(draft) => draft,
        None if unit.is_empty() => format!("{value}"),
        None => format!("{value} {unit}"),
    };

    let mut classes = String::from("knob");
    if inline {
        classes.push_str(" inline");
    }
    if disabled {
        classes.push_str(" disabled");
    }

    let body_id = id.clone();
    let value_id = format!("{id}-value");
    let value_id_focus = value_id.clone();
    let value_id_key = value_id.clone();

    let mut commit = move || {
        if let Some(draft) = editing() {
            let cleaned: String = draft
                .replace(',', ".")
                .chars()
                .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-'))
                .collect();
            if let Ok(n) = cleaned.parse::<f64>() {
                on_change.call(clamp_q(n));
            }
        }
        editing.set(None);
    };

    rsx! {
        div { class: "{classes}", title: if title.is_empty() { None } else { Some(title.clone()) },
            span { class: "knob-label", "{label}" }
            button {
                r#type: "button",
                id: "{body_id}",
                class: "knob-body",
                "aria-label": "{label}",
                disabled,
                onpointerdown: move |e| {
                    if disabled {
                        return;
                    }
                    e.prevent_default();
                    drag.set(Some((e.client_coordinates().y, value)));
                    let _ = document::eval(&format!(
                        "document.getElementById('{body_id}')?.setPointerCapture({});",
                        e.pointer_id()
                    ));
                },
                onpointermove: move |e| {
                    if let Some((start_y, start_value)) = drag() {
                        let travel = (start_y - e.client_coordinates().y) / 150.0;
                        on_change.call(clamp_q(start_value + travel * (max - min)));
                    }
                },
                onpointerup: move |_| drag.set(None),
                onpointercancel: move |_| drag.set(None),
                if arc {
                    svg { class: "knob-arc", view_box: "0 0 30 30", "aria-hidden": "true",
                        circle {
                            cx: "15",
                            cy: "15",
                            r: "9",
                            path_length: "360",
                            stroke_dasharray: "{270.0 * frac} 360",
                            transform: "rotate(135 15 15)",
                        }
                    }
                } else {
                    span { class: "knob-tick", style: "transform: rotate({angle}deg)" }
                }
            }
            input {
                id: "{value_id}",
                class: "knob-value",
                disabled,
                value: "{shown}",
                onfocus: move |_| {
                    editing.set(Some(format!("{value}")));
                    let _ = document::eval(&format!(
                        "const el=document.getElementById('{value_id_focus}'); requestAnimationFrame(()=>el&&el.select());"
                    ));
                },
                oninput: move |e| editing.set(Some(e.value())),
                onblur: move |_| commit(),
                onkeydown: move |e| match e.key() {
                    Key::Enter => {
                        commit();
                        let _ = document::eval(&format!("document.getElementById('{value_id_key}')?.blur();"));
                    }
                    Key::Escape => {
                        editing.set(None);
                        let _ = document::eval(&format!("document.getElementById('{value_id_key}')?.blur();"));
                    }
                    _ => {}
                },
            }
        }
    }
}
