// twin: ui-ctl-popover.jsx jsx@7bdc5084e035 rsx@cb478077722b
//! Popover — twin of `ui-ctl-popover.jsx`: the ONE floating-panel MECHANISM of the popover family. It
//! positions a `.pop-surface` against an anchor element: below it (flipping above when there is no
//! room), width floored at the anchor (optional) and capped, anchored to the left or right edge so it
//! stays on screen, repositioned on scroll/resize, closed on Esc and on a click outside. It owns
//! POSITION + DISMISS only, not the panel's contents. `Menu`, `Combobox`, `ColorPicker` and `TableGrid`
//! are built on it.
//!
//! `placement: Side` puts the panel BESIDE the anchor (a submenu flyout), flipping to the other side
//! when there is no room. `ignore_selector` exempts a selector from click-away, so a child panel does
//! not dismiss its parent out from under a click. `on_mouse_enter` / `on_mouse_leave` let a
//! hover-opened flyout stay open while the pointer is over itself.
//!
//! The anchor is named by its element id (`anchor_id`): the caller gives its trigger an `id`.
//! `.pop-surface` is `position: fixed`, so it needs no portal; the placement MATH is Rust (below), and
//! the page is only asked for two things rsx cannot do: the anchor's box, and the document-level events
//! (a press outside, Esc, scroll, resize). That is the one place this control touches the JS bridge.
//! Differs: behaviour none. Form only: the trigger is named by id (`anchor_id`) because rsx has no element refs, and
//! the panel is not portalled (`.pop-surface` is `position: fixed`, which needs none).

use dioxus::document::Eval;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

const MARGIN: f64 = 8.0; // breathing room kept between the panel and the window edge
const MIN_PANEL: f64 = 160.0; // never squeeze a panel below this, even in a cramped window
const FLIP_BELOW: f64 = 240.0; // less room than this under the anchor -> consider opening upwards

/// Which edge of the anchor a `Below` panel lines up with. `Auto` = right-half anchors right, else left.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum PopAlign {
    #[default]
    Auto,
    Left,
    Right,
}

/// `Below` = a dropdown under its anchor. `Side` = a submenu flyout beside its row.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum PopPlacement {
    #[default]
    Below,
    Side,
}

/// What the page-side bridge needs to know, resent whenever it changes.
#[derive(Clone, PartialEq, Serialize)]
struct Cfg {
    open: bool,
    anchor: String,
    ignore: String,
}

/// A message from the page: the anchor's box and the window's size.
#[derive(Deserialize, Default)]
struct Msg {
    #[serde(default)]
    t: String,
    #[serde(default)]
    l: f64,
    #[serde(default)]
    tp: f64,
    #[serde(default)]
    r: f64,
    #[serde(default)]
    b: f64,
    #[serde(default)]
    w: f64,
    #[serde(default)]
    ww: f64,
    #[serde(default)]
    wh: f64,
}

#[derive(Clone, Copy, PartialEq)]
struct Rect {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    width: f64,
    win_w: f64,
    win_h: f64,
}

/// The page side: runs for the popover's whole life, reports the anchor's box (on open, scroll and
/// resize) and a press outside / Esc while it is open. `anchor` is looked up by id each time, so a
/// trigger that re-renders is still found.
const BRIDGE: &str = r#"
let cfg = null;
const place = () => {
  if (!cfg || !cfg.open) return;
  const a = document.getElementById(cfg.anchor);
  if (!a) return;
  const r = a.getBoundingClientRect();
  dioxus.send({ t: 'rect', l: r.left, tp: r.top, r: r.right, b: r.bottom, w: r.width,
                ww: window.innerWidth, wh: window.innerHeight });
};
const onDown = (e) => {
  if (!cfg || !cfg.open) return;
  const a = document.getElementById(cfg.anchor);
  if (a && a.contains(e.target)) return;
  if (e.target && e.target.closest && e.target.closest('.pop-surface')) {
    // inside SOME panel; it is ours only if the press is inside the panel that follows this anchor
    const mine = document.querySelector('.pop-surface[data-anchor="' + cfg.anchor + '"]');
    if (mine && mine.contains(e.target)) return;
  }
  if (cfg.ignore && e.target && e.target.closest && e.target.closest(cfg.ignore)) return;
  dioxus.send({ t: 'close' });
};
const onKey = (e) => { if (cfg && cfg.open && e.key === 'Escape') dioxus.send({ t: 'close' }); };
document.addEventListener('mousedown', onDown);
document.addEventListener('keydown', onKey);
window.addEventListener('scroll', place, true);
window.addEventListener('resize', place);
try {
  while (true) {
    const m = await dioxus.recv();
    if (m && m.cmd === 'dispose') break;
    cfg = m;
    place();
  }
} finally {
  document.removeEventListener('mousedown', onDown);
  document.removeEventListener('keydown', onKey);
  window.removeEventListener('scroll', place, true);
  window.removeEventListener('resize', place);
}
"#;

/// The inline style of the panel: the JSX's placement math, line for line.
#[allow(clippy::too_many_arguments)]
fn placement_style(
    r: Rect,
    align: PopAlign,
    placement: PopPlacement,
    min_width_anchor: bool,
    max_width: f64,
    max_height: Option<f64>,
) -> String {
    let mw = max_width.min(r.win_w - 16.0);

    // The height cap is the ROOM ON SCREEN, not a constant; only an explicit `max_height` caps a panel
    // below the room on purpose (a long list stays a short list).
    let room_below = r.win_h - r.bottom - MARGIN * 2.0;
    let room_above = r.top - MARGIN * 2.0;
    let fit = |room: f64| match max_height {
        Some(h) => MIN_PANEL.max(h.min(room)),
        None => MIN_PANEL.max(room),
    };

    // 'side' - a submenu opens BESIDE its parent row and flips to the other side when there is no room.
    // Edges touch deliberately (no gap), so the pointer can travel from the row into the flyout.
    if placement == PopPlacement::Side {
        let flip_left = r.right + mw + MARGIN > r.win_w && r.left > mw;
        // A flyout hangs DOWN from its row, so its room is everything below that row's top.
        let cap = fit(r.win_h - r.top - MARGIN * 2.0);
        let horizontal = if flip_left {
            format!("right: {}px;", MARGIN.max(r.win_w - r.left))
        } else {
            format!("left: {}px;", r.right)
        };
        // -4 cancels .pop-surface's padding so the first row lines up with the parent row.
        let top = MARGIN.max((r.top - 4.0).min(r.win_h - cap - MARGIN));
        return format!("max-width: {mw}px; max-height: {cap}px; {horizontal} top: {top}px;");
    }

    // Flip up only when below is genuinely cramped AND above is roomier.
    let go_up = room_below < max_height.unwrap_or(FLIP_BELOW).min(FLIP_BELOW) && room_above > room_below;
    let anchor_right = align == PopAlign::Right || (align == PopAlign::Auto && r.left > r.win_w / 2.0);
    let cap = fit(if go_up { room_above } else { room_below });
    let min_w = if min_width_anchor { format!("min-width: {}px; ", r.width) } else { String::new() };
    let horizontal = if anchor_right {
        format!("right: {}px;", MARGIN.max(r.win_w - r.right))
    } else {
        format!("left: {}px;", r.left)
    };
    let vertical = if go_up {
        format!("bottom: {}px;", r.win_h - r.top + 4.0)
    } else {
        format!("top: {}px;", r.bottom + 4.0)
    };
    format!("max-width: {mw}px; max-height: {cap}px; {min_w}{horizontal} {vertical}")
}

#[component]
pub fn Popover(
    /// The `id` of the trigger element the panel is placed against.
    anchor_id: String,
    open: bool,
    on_close: EventHandler<()>,
    #[props(default)] align: PopAlign,
    #[props(default)] placement: PopPlacement,
    /// Floor the panel width at the anchor's width (pickers do; menus don't).
    #[props(default = false)] min_width_anchor: bool,
    #[props(default = 400.0)] max_width: f64,
    /// `None` = fit the viewport (cap at the room on the chosen side).
    #[props(default)] max_height: Option<f64>,
    /// Click-away exception: a press inside this selector does NOT dismiss.
    #[props(default)] ignore_selector: String,
    #[props(default)] on_mouse_enter: Option<EventHandler<()>>,
    #[props(default)] on_mouse_leave: Option<EventHandler<()>>,
    #[props(default)] class: String,
    children: Element,
) -> Element {
    let pop_id = use_hook(|| format!("pop-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed)));
    let mut rect = use_signal(|| Option::<Rect>::None);
    // The bridge handle, outside any signal so the cleanup below can still reach it as the component
    // goes away (`Eval::send` on a finished eval is an error, never a panic).
    let bridge = use_hook(|| Rc::new(Cell::new(Option::<Eval>::None)));
    let latest_cfg = use_hook(|| Rc::new(std::cell::RefCell::new(Option::<Cfg>::None)));
    let close = use_callback(move |_: ()| on_close.call(()));

    {
        let bridge = bridge.clone();
        let latest_cfg = latest_cfg.clone();
        use_future(move || {
            let bridge = bridge.clone();
            let latest_cfg = latest_cfg.clone();
            async move {
                let mut ev = document::eval(BRIDGE);
                bridge.set(Some(ev));
                if let Some(c) = latest_cfg.borrow().clone() {
                    let _ = ev.send(&c);
                }
                while let Ok(msg) = ev.recv::<Msg>().await {
                    match msg.t.as_str() {
                        "rect" => rect.set(Some(Rect {
                            left: msg.l,
                            top: msg.tp,
                            right: msg.r,
                            bottom: msg.b,
                            width: msg.w,
                            win_w: msg.ww,
                            win_h: msg.wh,
                        })),
                        "close" => close.call(()),
                        _ => {}
                    }
                }
            }
        });
    }
    {
        let bridge = bridge.clone();
        use_drop(move || {
            if let Some(ev) = bridge.get() {
                let _ = ev.send(serde_json::json!({ "cmd": "dispose" }));
            }
        });
    }

    let cfg = Cfg { open, anchor: anchor_id.clone(), ignore: ignore_selector.clone() };
    {
        let bridge = bridge.clone();
        let latest_cfg = latest_cfg.clone();
        use_effect(use_reactive!(|(cfg,)| {
            *latest_cfg.borrow_mut() = Some(cfg.clone());
            if !cfg.open {
                rect.set(None);
            }
            if let Some(ev) = bridge.get() {
                let _ = ev.send(&cfg);
            }
        }));
    }

    let Some(r) = rect() else { return rsx! {} };
    if !open {
        return rsx! {};
    }
    let style = placement_style(r, align, placement, min_width_anchor, max_width, max_height);
    let extra = if class.is_empty() { String::new() } else { format!(" {class}") };

    rsx! {
        div {
            id: "{pop_id}",
            class: "pop-surface{extra}",
            "data-anchor": "{anchor_id}",
            style: "{style}",
            onmouseenter: move |_| {
                if let Some(h) = on_mouse_enter {
                    h.call(());
                }
            },
            onmouseleave: move |_| {
                if let Some(h) = on_mouse_leave {
                    h.call(());
                }
            },
            {children}
        }
    }
}

// ── Dismiss-only helper, for panels that place themselves ─────────────────────────────────────────────

#[derive(Clone, PartialEq, Serialize)]
struct DismissCfg {
    open: bool,
    anchor: String,
    panel: String,
}

/// The page side of `use_dismiss`: a press outside both the anchor and the panel, or Esc, while open.
const DISMISS_BRIDGE: &str = r#"
let cfg = null;
const onDown = (e) => {
  if (!cfg || !cfg.open) return;
  const a = document.getElementById(cfg.anchor);
  const p = document.getElementById(cfg.panel);
  if ((a && a.contains(e.target)) || (p && p.contains(e.target))) return;
  dioxus.send({ t: 'close' });
};
const onKey = (e) => { if (cfg && cfg.open && e.key === 'Escape') dioxus.send({ t: 'close' }); };
document.addEventListener('mousedown', onDown);
document.addEventListener('keydown', onKey);
try {
  while (true) {
    const m = await dioxus.recv();
    if (m && m.cmd === 'dispose') break;
    cfg = m;
  }
} finally {
  document.removeEventListener('mousedown', onDown);
  document.removeEventListener('keydown', onKey);
}
"#;

/// Closes a panel that positions ITSELF (the date picker, the colour picker) on a press outside it and
/// its anchor, and on Esc - what `Popover` does for the panels it places. Call it once per component,
/// every render, with the current `open`; both elements are named by id.
pub(super) fn use_dismiss(open: bool, anchor_id: &str, panel_id: &str, on_close: Callback<()>) {
    let bridge = use_hook(|| Rc::new(Cell::new(Option::<Eval>::None)));
    let latest = use_hook(|| Rc::new(std::cell::RefCell::new(Option::<DismissCfg>::None)));

    {
        let bridge = bridge.clone();
        let latest = latest.clone();
        use_future(move || {
            let bridge = bridge.clone();
            let latest = latest.clone();
            async move {
                let mut ev = document::eval(DISMISS_BRIDGE);
                bridge.set(Some(ev));
                if let Some(c) = latest.borrow().clone() {
                    let _ = ev.send(&c);
                }
                while let Ok(msg) = ev.recv::<Msg>().await {
                    if msg.t == "close" {
                        on_close.call(());
                    }
                }
            }
        });
    }
    {
        let bridge = bridge.clone();
        use_drop(move || {
            if let Some(ev) = bridge.get() {
                let _ = ev.send(serde_json::json!({ "cmd": "dispose" }));
            }
        });
    }

    let cfg = DismissCfg { open, anchor: anchor_id.to_string(), panel: panel_id.to_string() };
    use_effect(use_reactive!(|(cfg,)| {
        *latest.borrow_mut() = Some(cfg.clone());
        if let Some(ev) = bridge.get() {
            let _ = ev.send(&cfg);
        }
    }));
}
