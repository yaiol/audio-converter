//! Equalizer — twin of `ui-ctl-equalizer.jsx`: a row of vertical gain sliders, one per band, with the
//! value above and the caption below. Same classes and element order, so `ui-ctl-equalizer.css` skins
//! it unchanged. Behaviour is the JSX's: grab a band and drag (the thumb goes where the pointer is,
//! quantised to `step`), arrow keys move one step, PageUp/PageDown three, Home/End the ends,
//! double-click puts a band back in the middle.
//!
//! Pointer capture has no Rust API, so it goes through `document::eval` by element id - the one
//! place this control touches the JS bridge. The band's box (its scale) comes from the mounted
//! element, not from a constant, so the CSS stays the only place its height is written.

use dioxus::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// Quantise to `step`, clamp to the range, and shed float noise (0.30000000000000004).
fn quantise(v: f64, min: f64, max: f64, step: f64) -> f64 {
    let q = ((v - min) / step).round() * step + min;
    ((q * 1e6).round() / 1e6).clamp(min, max)
}

fn shown(v: f64) -> String {
    if v > 0.0 { format!("+{v}") } else { format!("{v}") }
}

#[component]
pub fn Equalizer(
    labels: Vec<String>,
    values: Vec<f64>,
    #[props(default = -12.0)] min: f64,
    #[props(default = 12.0)] max: f64,
    #[props(default = 1.0)] step: f64,
    #[props(default)] unit: String,
    #[props(default)] label_unit: String,
    #[props(default = false)] disabled: bool,
    #[props(default)] title: String,
    /// `(band index, new value)` - only when the value actually changed.
    on_change: EventHandler<(usize, f64)>,
) -> Element {
    let id = use_hook(|| format!("eq-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed)));
    // The band being held: its index and its box (top, height) on screen, read when it was grabbed.
    let mut drag = use_signal(|| Option::<(usize, f64, f64)>::None);

    let span = if max > min { max - min } else { 1.0 };
    let mid = ((0.0 - min) / span).clamp(0.0, 1.0);

    let class = if disabled { "eq disabled" } else { "eq" };

    rsx! {
        div { class: "{class}", role: "group", title: if title.is_empty() { None } else { Some(title.clone()) },
            for (i, v) in values.iter().copied().enumerate() {
                {
                    let frac = ((v - min) / span).clamp(0.0, 1.0);
                    let (lo, hi) = (frac.min(mid), frac.max(mid));
                    let label = labels.get(i).cloned().unwrap_or_default();
                    let caption = if label_unit.is_empty() { label.clone() } else { format!("{label} {label_unit}") };
                    let text = if unit.is_empty() { shown(v) } else { format!("{} {unit}", shown(v)) };
                    let track_id = format!("{id}-{i}");
                    let track_id_down = track_id.clone();
                    rsx! {
                        div { class: "eq-band", key: "{i}",
                            span { class: "eq-value", "{shown(v)}" }
                            div {
                                id: "{track_id}",
                                class: "eq-track",
                                role: "slider",
                                tabindex: if disabled { "-1" } else { "0" },
                                "aria-orientation": "vertical",
                                "aria-label": "{caption}",
                                "aria-valuemin": "{min}",
                                "aria-valuemax": "{max}",
                                "aria-valuenow": "{v}",
                                "aria-valuetext": "{text}",
                                title: "{caption}: {text}",
                                onpointerdown: move |e| {
                                    if disabled {
                                        return;
                                    }
                                    e.prevent_default();
                                    let pointer = e.pointer_id();
                                    let y = e.client_coordinates().y;
                                    let tid = track_id_down.clone();
                                    // The band's box is asked of the page - it is the scale - then the
                                    // pointer's height on it is the value, and the drag carries on.
                                    spawn(async move {
                                        let mut ev = document::eval(&format!(
                                            "const el = document.getElementById('{tid}'); \
                                             if (el) {{ el.setPointerCapture({pointer}); el.focus(); \
                                             const r = el.getBoundingClientRect(); dioxus.send([r.top, r.height]); }} \
                                             else {{ dioxus.send([0, 0]); }}"
                                        ));
                                        if let Ok([top, height]) = ev.recv::<[f64; 2]>().await {
                                            if height > 0.0 {
                                                drag.set(Some((i, top, height)));
                                                let next = quantise(min + (1.0 - (y - top) / height) * span, min, max, step);
                                                if next != v {
                                                    on_change.call((i, next));
                                                }
                                            }
                                        }
                                    });
                                },
                                onpointermove: move |e| {
                                    if let Some((band, top, height)) = drag() {
                                        if band == i {
                                            let next = quantise(
                                                min + (1.0 - (e.client_coordinates().y - top) / height) * span,
                                                min,
                                                max,
                                                step,
                                            );
                                            if next != v {
                                                on_change.call((i, next));
                                            }
                                        }
                                    }
                                },
                                onpointerup: move |_| drag.set(None),
                                onpointercancel: move |_| drag.set(None),
                                ondoubleclick: move |_| {
                                    if !disabled {
                                        let mid_value = quantise(0.0, min, max, step);
                                        if mid_value != v {
                                            on_change.call((i, mid_value));
                                        }
                                    }
                                },
                                onkeydown: move |e| {
                                    if disabled {
                                        return;
                                    }
                                    let target = match e.key() {
                                        Key::ArrowUp | Key::ArrowRight => v + step,
                                        Key::ArrowDown | Key::ArrowLeft => v - step,
                                        Key::PageUp => v + 3.0 * step,
                                        Key::PageDown => v - 3.0 * step,
                                        Key::Home => min,
                                        Key::End => max,
                                        _ => return,
                                    };
                                    e.prevent_default();
                                    let next = quantise(target, min, max, step);
                                    if next != v {
                                        on_change.call((i, next));
                                    }
                                },
                                span { class: "eq-rail" }
                                span { class: "eq-zero", style: "bottom: {mid * 100.0}%" }
                                span { class: "eq-fill", style: "bottom: {lo * 100.0}%; height: {(hi - lo) * 100.0}%" }
                                span { class: "eq-thumb", style: "bottom: {frac * 100.0}%" }
                            }
                            span { class: "eq-label", "{label}" }
                        }
                    }
                }
            }
        }
    }
}
