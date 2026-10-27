// twin: ui-ctl-colorpicker.jsx jsx@08cf1133c966 rsx@93b7353b3e59
//! ColorPicker — twin of `ui-ctl-colorpicker.jsx`: the colour picker. A swatch button that opens a
//! draggable popup: a saturation/hue field, an eyedropper (when the platform has one), a hex input,
//! R/G/B number fields, a row of quick swatches, and Cancel / Apply. Same classes and element order as
//! the JSX; it borrows the shared field classes (`.dlg-form` / `.input` / `.btn.icon` / `.btn`) and the
//! shared `NumberField`.
//!
//! Controlled: `color` (hex string) + `on_change(hex)` - `on_change` fires on Apply only, so the caller
//! commits once. The labels are props, so the control carries no i18n key coupling.
//!
//! The JSX uses `react-colorful` for the saturation/hue field. There is no such library here, so the
//! field is built in rsx with the SAME element and class names (`react-colorful*`) and the same look;
//! the field's pointer capture goes through `document::eval` by element id, and dragging the popup by
//! its grip uses a transparent overlay while the button is held - no page-wide listeners.
//! Differs: behaviour none. Form only: the saturation/hue field is built here (the JSX uses react-colorful) under the
//! same class names and look, and dragging the popup uses a transparent sheet while the button is held instead of
//! document listeners.

use super::numberfield::NumberField;
use super::popover::use_dismiss;
use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdGripHorizontal, LdPipette},
};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

const COLOR_SWATCHES: [&str; 7] = ["#ff6eb4", "#f0c040", "#60a5fa", "#7c6fff", "#4dc8c8", "#5dba6f", "#e06060"];
const POPUP_H: f64 = 400.0;

/// The field's own look: the stylesheet `react-colorful` puts in the page, under the same class names.
const FIELD_CSS: &str = ".react-colorful{position:relative;display:flex;flex-direction:column;width:200px;height:200px;user-select:none;cursor:default}\
.react-colorful__saturation{position:relative;flex-grow:1;border-color:transparent;border-bottom:12px solid #000;border-radius:8px 8px 0 0;background-image:linear-gradient(to top,#000,rgba(0,0,0,0)),linear-gradient(to right,#fff,rgba(255,255,255,0))}\
.react-colorful__hue{position:relative;height:24px;background:linear-gradient(to right,#f00 0,#ff0 17%,#0f0 33%,#0ff 50%,#00f 67%,#f0f 83%,#f00 100%)}\
.react-colorful__last-control{border-radius:0 0 8px 8px}\
.react-colorful__interactive{position:absolute;left:0;top:0;right:0;bottom:0;border-radius:inherit;outline:none;touch-action:none}\
.react-colorful__pointer{position:absolute;z-index:1;box-sizing:border-box;width:28px;height:28px;transform:translate(-50%,-50%);background-color:#fff;border:2px solid #fff;border-radius:50%;box-shadow:0 2px 4px rgba(0,0,0,.2)}\
.react-colorful__interactive:focus .react-colorful__pointer{transform:translate(-50%,-50%) scale(1.1)}\
.react-colorful__pointer-fill{display:block;width:100%;height:100%;border-radius:inherit}";

// ── Colour arithmetic ──────────────────────────────────────────────────────────────────────────────────

fn valid_hex(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn hex_to_rgb(h: &str) -> (u32, u32, u32) {
    let p = |i: usize| u32::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
    (p(1), p(3), p(5))
}

fn rgb_to_hex(r: u32, g: u32, b: u32) -> String {
    format!("#{:02x}{:02x}{:02x}", r.min(255), g.min(255), b.min(255))
}

/// h 0-360, s 0-100, v 0-100.
fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u32, u32, u32) {
    let (s, v) = (s / 100.0, v / 100.0);
    let c = v * s;
    let hh = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hh % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hh as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to = |f: f64| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    (to(r1), to(g1), to(b1))
}

fn hsv_to_hex(h: f64, s: f64, v: f64) -> String {
    let (r, g, b) = hsv_to_rgb(h, s, v);
    rgb_to_hex(r, g, b)
}

fn hex_to_hsv(hex: &str) -> (f64, f64, f64) {
    let (r, g, b) = hex_to_rgb(hex);
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max * 100.0 };
    (h.rem_euclid(360.0), s, max * 100.0)
}

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Field,
    Hue,
}

#[component]
pub fn ColorPicker(
    color: String,
    on_change: EventHandler<String>,
    #[props(default)] cancel_label: String,
    #[props(default)] apply_label: String,
    #[props(default)] pick_title: String,
) -> Element {
    let id = use_hook(|| NEXT_ID.fetch_add(1, Ordering::Relaxed));
    let btn_id = format!("cp-btn-{id}");
    let pop_id = format!("cp-pop-{id}");
    let sat_id = format!("cp-sat-{id}");
    let hue_id = format!("cp-hue-{id}");

    let mut open = use_signal(|| false);
    let mut draft = use_signal(|| color.clone());
    let mut hex = use_signal(|| color.clone());
    let mut hsv = use_signal(|| (0.0, 0.0, 0.0));
    let mut pop_pos = use_signal(|| (0.0f64, 0.0f64));
    // The grip is held: where inside the popup it was grabbed.
    let mut moving = use_signal(|| Option::<(f64, f64)>::None);
    // A field/hue strip is held: which, and its box (left, top, width, height) read when it was grabbed.
    let mut drag = use_signal(|| Option::<(Part, f64, f64, f64, f64)>::None);
    let mut has_eyedropper = use_signal(|| false);

    let close = use_callback(move |_: ()| open.set(false));
    use_dismiss(open(), &btn_id, &pop_id, close);

    use_future(move || async move {
        if let Ok(yes) = document::eval("dioxus.send(!!window.EyeDropper);").recv::<bool>().await {
            has_eyedropper.set(yes);
        }
    });

    // The field keeps its own HSV so a grey (s = 0) does not lose its hue; it is re-read from the hex only
    // when the hex was changed from outside the field (typing, the RGB boxes, a swatch).
    use_effect(move || {
        let d = draft();
        if valid_hex(&d) {
            let (h, s, v) = *hsv.peek();
            if hsv_to_hex(h, s, v) != d.to_lowercase() {
                hsv.set(hex_to_hsv(&d));
            }
        }
    });

    let mut update_draft = move |c: String| {
        draft.set(c.clone());
        hex.set(c);
    };
    let mut set_hsv = move |h: f64, s: f64, v: f64| {
        hsv.set((h, s, v));
        let c = hsv_to_hex(h, s, v);
        draft.set(c.clone());
        hex.set(c);
    };

    let toggle = {
        let btn_id = btn_id.clone();
        let color = color.clone();
        move |_| {
            if open() {
                open.set(false);
                return;
            }
            let btn_id = btn_id.clone();
            let color = color.clone();
            spawn(async move {
                let mut ev = document::eval(&format!(
                    "const el = document.getElementById('{btn_id}'); \
                     if (el) {{ const r = el.getBoundingClientRect(); dioxus.send([r.top, r.bottom, r.left, window.innerHeight]); }} \
                     else {{ dioxus.send([0, 0, 0, 0]); }}"
                ));
                if let Ok([top, bottom, left, win_h]) = ev.recv::<[f64; 4]>().await {
                    let go_up = bottom + POPUP_H > win_h;
                    pop_pos.set(if go_up { (top - POPUP_H - 6.0, left) } else { (bottom + 6.0, left) });
                    draft.set(color.clone());
                    hex.set(color.clone());
                    if valid_hex(&color) {
                        hsv.set(hex_to_hsv(&color));
                    }
                    open.set(true);
                }
            });
        }
    };

    // Grab a strip: ask the page for its box and pin the pointer to it, then the position is the value.
    let grab = move |part: Part, el_id: String, e: Event<PointerData>| {
        e.prevent_default();
        let pointer = e.pointer_id();
        let (x, y) = (e.client_coordinates().x, e.client_coordinates().y);
        spawn(async move {
            let mut ev = document::eval(&format!(
                "const el = document.getElementById('{el_id}'); \
                 if (el) {{ el.setPointerCapture({pointer}); el.focus(); const r = el.getBoundingClientRect(); \
                 dioxus.send([r.left, r.top, r.width, r.height]); }} else {{ dioxus.send([0, 0, 0, 0]); }}"
            ));
            if let Ok([l, t, w, h]) = ev.recv::<[f64; 4]>().await {
                if w > 0.0 && h > 0.0 {
                    drag.set(Some((part, l, t, w, h)));
                    let (hh, ss, vv) = hsv();
                    match part {
                        Part::Field => set_hsv(hh, ((x - l) / w).clamp(0.0, 1.0) * 100.0, 100.0 - ((y - t) / h).clamp(0.0, 1.0) * 100.0),
                        Part::Hue => set_hsv(((x - l) / w).clamp(0.0, 1.0) * 360.0, ss, vv),
                    }
                }
            }
        });
    };
    let mut drag_to = move |part: Part, e: Event<PointerData>| {
        if let Some((p, l, t, w, h)) = drag() {
            if p == part {
                let (x, y) = (e.client_coordinates().x, e.client_coordinates().y);
                let (hh, ss, vv) = hsv();
                match part {
                    Part::Field => set_hsv(hh, ((x - l) / w).clamp(0.0, 1.0) * 100.0, 100.0 - ((y - t) / h).clamp(0.0, 1.0) * 100.0),
                    Part::Hue => set_hsv(((x - l) / w).clamp(0.0, 1.0) * 360.0, ss, vv),
                }
            }
        }
    };
    // Arrow keys nudge the held strip by one unit, as the library's do.
    let mut nudge = move |part: Part, e: Event<KeyboardData>| {
        let (dx, dy) = match e.key() {
            Key::ArrowLeft => (-1.0, 0.0),
            Key::ArrowRight => (1.0, 0.0),
            Key::ArrowUp => (0.0, 1.0),
            Key::ArrowDown => (0.0, -1.0),
            _ => return,
        };
        e.prevent_default();
        let (h, s, v) = hsv();
        match part {
            Part::Field => set_hsv(h, (s + dx).clamp(0.0, 100.0), (v + dy).clamp(0.0, 100.0)),
            Part::Hue => set_hsv((h + dx).clamp(0.0, 360.0), s, v),
        }
    };

    let (draft_now, hex_now) = (draft(), hex());
    let rgb = if valid_hex(&draft_now) { hex_to_rgb(&draft_now) } else { (0, 0, 0) };
    let mut handle_rgb = move |channel: usize, val: Option<u32>| {
        let Some(n) = val else { return };
        let d = draft();
        let mut c = if valid_hex(&d) { hex_to_rgb(&d) } else { (0, 0, 0) };
        let n = n.min(255);
        match channel {
            0 => c.0 = n,
            1 => c.1 = n,
            _ => c.2 = n,
        }
        update_draft(rgb_to_hex(c.0, c.1, c.2));
    };

    let (h, s, v) = hsv();
    let sat_pointer = hsv_to_hex(h, s, v);
    let hue_color = hsv_to_hex(h, 100.0, 100.0);
    let swatch_shadow = |on: bool, c: &str| if on { format!("box-shadow: 0 0 0 2px {c}") } else { String::new() };
    let (top, left) = pop_pos();

    rsx! {
        div { style: "display: inline-block; flex-shrink: 0",
            button {
                id: "{btn_id}",
                r#type: "button",
                class: if open() { "swatch on" } else { "swatch" },
                style: "background: {color}; {swatch_shadow(open(), &color)}",
                onclick: toggle,
            }
            if open() {
                document::Style { "{FIELD_CSS}" }
                div {
                    id: "{pop_id}",
                    class: "dlg-form",
                    style: "position: fixed; z-index: 9999; top: {top}px; left: {left}px; background: var(--dlg-bgd); border: 1px solid var(--border); border-radius: 10px; box-shadow: 0 8px 32px #0008; display: flex; flex-direction: column; width: 220px; overflow: hidden",
                    div {
                        style: "cursor: grab; display: flex; align-items: center; justify-content: center; padding: 6px 0 4px; border-bottom: 1px solid var(--border); color: var(--text-dim); flex-shrink: 0",
                        onmousedown: move |e| {
                            e.prevent_default();
                            let (top, left) = pop_pos();
                            moving.set(Some((e.client_coordinates().x - left, e.client_coordinates().y - top)));
                        },
                        Icon { class: "icon-inline", icon: LdGripHorizontal }
                    }
                    div { style: "display: flex; flex-direction: column; gap: 10px; padding: 12px",
                        div { class: "react-colorful", style: "width: 100%",
                            div {
                                class: "react-colorful__saturation",
                                style: "background-color: {hue_color}",
                                div {
                                    id: "{sat_id}",
                                    class: "react-colorful__interactive",
                                    tabindex: "0",
                                    role: "slider",
                                    "aria-label": "Color",
                                    onpointerdown: {
                                        let sat_id = sat_id.clone();
                                        move |e| grab(Part::Field, sat_id.clone(), e)
                                    },
                                    onpointermove: move |e| drag_to(Part::Field, e),
                                    onpointerup: move |_| drag.set(None),
                                    onpointercancel: move |_| drag.set(None),
                                    onkeydown: move |e| nudge(Part::Field, e),
                                    div {
                                        class: "react-colorful__pointer react-colorful__saturation-pointer",
                                        style: "top: {100.0 - v}%; left: {s}%",
                                        div { class: "react-colorful__pointer-fill", style: "background-color: {sat_pointer}" }
                                    }
                                }
                            }
                            div { class: "react-colorful__hue react-colorful__last-control",
                                div {
                                    id: "{hue_id}",
                                    class: "react-colorful__interactive",
                                    tabindex: "0",
                                    role: "slider",
                                    "aria-label": "Hue",
                                    onpointerdown: {
                                        let hue_id = hue_id.clone();
                                        move |e| grab(Part::Hue, hue_id.clone(), e)
                                    },
                                    onpointermove: move |e| drag_to(Part::Hue, e),
                                    onpointerup: move |_| drag.set(None),
                                    onpointercancel: move |_| drag.set(None),
                                    onkeydown: move |e| nudge(Part::Hue, e),
                                    div {
                                        class: "react-colorful__pointer react-colorful__hue-pointer",
                                        style: "left: {h / 360.0 * 100.0}%",
                                        div { class: "react-colorful__pointer-fill", style: "background-color: {hue_color}" }
                                    }
                                }
                            }
                        }
                        div { style: "display: flex; gap: 6px; align-items: center",
                            if has_eyedropper() {
                                button {
                                    r#type: "button",
                                    class: "btn icon",
                                    title: if pick_title.is_empty() { None } else { Some(pick_title.clone()) },
                                    onclick: move |_| {
                                        spawn(async move {
                                            let mut ev = document::eval(
                                                "try { const r = await new window.EyeDropper().open(); dioxus.send(r.sRGBHex); } catch (_) { dioxus.send(''); }",
                                            );
                                            if let Ok(c) = ev.recv::<String>().await {
                                                if valid_hex(&c) {
                                                    update_draft(c);
                                                }
                                            }
                                        });
                                    },
                                    Icon { width: 16, height: 16, icon: LdPipette }
                                }
                            }
                            input {
                                class: "input",
                                style: "flex: 1; font-family: monospace",
                                maxlength: "7",
                                value: "{hex_now}",
                                oninput: move |e| {
                                    let v = e.value();
                                    hex.set(v.clone());
                                    if valid_hex(&v) {
                                        draft.set(v);
                                    }
                                },
                                onblur: move |_| {
                                    if !valid_hex(&hex()) {
                                        hex.set(draft());
                                    }
                                },
                            }
                        }
                        div { style: "display: flex; gap: 6px; justify-content: space-between",
                            for (ch, (name, value)) in [("r", rgb.0), ("g", rgb.1), ("b", rgb.2)].into_iter().enumerate() {
                                div {
                                    key: "{name}",
                                    style: "display: flex; flex-direction: column; gap: 2px; align-items: center",
                                    NumberField {
                                        min: Some(0.0),
                                        max: Some(255.0),
                                        value: Some(value as f64),
                                        width: "56px",
                                        on_change: move |val: Option<f64>| handle_rgb(ch, val.map(|v| v.round().clamp(0.0, 255.0) as u32)),
                                    }
                                    span { style: "font-size: 10px; color: var(--text-dim); text-transform: uppercase", "{name}" }
                                }
                            }
                        }
                        div { style: "display: flex; flex-wrap: wrap; gap: 5px",
                            for c in COLOR_SWATCHES {
                                button {
                                    key: "{c}",
                                    r#type: "button",
                                    class: if draft_now == c { "swatch small on" } else { "swatch small" },
                                    style: "background: {c}; {swatch_shadow(draft_now == c, c)}",
                                    onclick: move |_| update_draft(c.to_string()),
                                }
                            }
                        }
                        div { style: "display: flex; gap: 8px",
                            button {
                                r#type: "button",
                                class: "btn subtle",
                                style: "flex: 1",
                                onclick: move |_| open.set(false),
                                "{cancel_label}"
                            }
                            button {
                                r#type: "button",
                                class: "btn primary",
                                style: "flex: 1",
                                onclick: move |_| {
                                    on_change.call(draft());
                                    open.set(false);
                                },
                                "{apply_label}"
                            }
                        }
                    }
                }
                // While the grip is held the pointer is followed by a transparent sheet over the window,
                // so the popup keeps moving however fast the pointer leaves it.
                if moving().is_some() {
                    div {
                        style: "position: fixed; inset: 0; z-index: 10000; cursor: grabbing; user-select: none",
                        onmousemove: move |e| {
                            if let Some((ox, oy)) = moving() {
                                pop_pos.set((e.client_coordinates().y - oy, e.client_coordinates().x - ox));
                            }
                        },
                        onmouseup: move |_| moving.set(None),
                    }
                }
            }
        }
    }
}
