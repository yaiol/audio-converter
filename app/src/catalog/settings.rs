// twin: ui-settings.jsx jsx@92a48344142a rsx@d2421823136f
//! SettingsView — twin of `ui-settings.jsx`: the full-window settings page. Same classes and
//! element structure (rail + page, `.tabs.vertical`, `.dlg-field`, `.opt-btns`, `.dlg-about`).
//!
//! Ported: frame, General (Language + Theme), About, app-supplied `tabs` (+ the page of the active
//! one), `general_extra` / `about_extra`, the `stg-open` class on <html>.
//! The page also owns the KEYBOARD as it owns the mouse: no app shortcut can act on the view it covers, focus is
//! kept inside it, Tab cycles, Escape leaves (or closes a popup opened from a tab first). That logic is DOM and
//! document-level, so it runs in the page, copied from the JSX's effect (`KEYS_JS`).
//! Differs: behaviour none. Form only: an app's tab pages that must stay loaded are passed as `app_pages` (one per
//! tab key) where the JSX takes `content` inside each tab; the older `app_page` route (the active tab's page,
//! supplied on demand) is kept for apps that use it and reloads that page on each visit.

use crate::i18n::{self, LANGUAGES};
use dioxus::document::Eval;
use dioxus::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::LazyLock;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdArrowLeft, LdMoon, LdScrollText, LdSun},
};

// Embedded as a data URI, not `asset!`: that macro resolves only under the `dx` CLI, and a plain
// `cargo build` executable must carry its own logo.
static LOGO: LazyLock<String> = LazyLock::new(|| {
    use base64::{Engine, engine::general_purpose::STANDARD};
    format!("data:image/svg+xml;base64,{}", STANDARD.encode(include_bytes!("../../assets/yaiol-logo.svg")))
});

/// A tab the APP adds between General and About. `key` is the app's own identifier for it.
#[derive(Clone, PartialEq)]
pub struct AppTab {
    pub key: String,
    pub label: String,
    pub icon: Element,
}

static NEXT_VIEW: AtomicU32 = AtomicU32::new(1);

/// The keyboard half of the page, copied from the JSX's effect: every key event is stopped on <body> - AFTER
/// the focused control has had it (so typing in a field works), BEFORE the app's own document/window
/// shortcuts - so no app shortcut can act on the view the page covers. Focus moves into the page and Tab is
/// kept inside it. Escape leaves the page; while a popup opened from a tab is up (`.pop-surface`, `.dp-pop`:
/// they close on their own document listener) Escape closes that popup instead.
const KEYS_JS: &str = r#"
let view = null;
for (let i = 0; i < 40 && !view; i++) {
  view = document.getElementById(VIEW_ID);
  if (!view) await new Promise((r) => setTimeout(r, 25));
}
if (view) {
  view.querySelector('.tab.active')?.focus();
  const POPUP = '.pop-surface, .dp-pop';
  const focusables = () => [...view.querySelectorAll('button, input, select, textarea, a[href], [tabindex]:not([tabindex="-1"])')]
    .filter((el) => !el.disabled && el.offsetParent !== null);
  const onKey = (e) => {
    const popup = document.querySelector(POPUP);
    if (popup && e.key === 'Escape') return;   // on to document, where the popup closes itself
    e.stopPropagation();
    if (e.type !== 'keydown') return;
    if (e.key === 'Escape') { dioxus.send('close'); return; }
    if (e.key !== 'Tab') return;
    const els = focusables();
    if (!els.length) return;
    const first = els[0], last = els[els.length - 1], at = document.activeElement;
    if (!view.contains(at)) { e.preventDefault(); first.focus(); }
    else if (e.shiftKey && at === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && at === last) { e.preventDefault(); first.focus(); }
  };
  // The Escape a popup was given stops at document: the popup's own document listener still runs, the app's
  // window listeners do not.
  const onDocKey = (e) => { if (e.key === 'Escape' && document.querySelector(POPUP)) e.stopPropagation(); };
  const TYPES = ['keydown', 'keyup', 'keypress'];
  TYPES.forEach((t) => { document.body.addEventListener(t, onKey); document.addEventListener(t, onDocKey); });
  try {
    while (true) { const m = await dioxus.recv(); if (m === 'dispose') break; }
  } finally {
    TYPES.forEach((t) => { document.body.removeEventListener(t, onKey); document.removeEventListener(t, onDocKey); });
  }
}
"#;

const GENERAL: &str = "general";
const ABOUT: &str = "about";

#[component]
pub fn SettingsView(
    app_name: String,
    app_version: String,
    lang: String,
    theme: String,
    on_set_lang: EventHandler<String>,
    on_set_theme: EventHandler<String>,
    on_close: EventHandler<()>,
    /// Tabs the app adds, shown between General and About.
    #[props(default)]
    app_tabs: Vec<AppTab>,
    /// The tab to open on: `general`, `about`, or an app tab's key. Empty = General.
    #[props(default)]
    initial_tab: String,
    /// Told the key of the tab that became active (and of the one it opened on), so the app can
    /// supply `app_page` for it.
    #[props(default)]
    on_tab: Option<EventHandler<String>>,
    /// The page of the active APP tab. Ignored while a built-in tab is active.
    #[props(default)]
    app_page: Option<Element>,
    /// The pages of app tabs that must stay LOADED once visited, as the JSX keeps every visited tab mounted: a tab's own
    /// state survives switching away and back, and a heavy tab costs nothing until it is opened. Keyed by tab key; a tab
    /// with no entry here falls back to `app_page` (supplied for the active tab only).
    #[props(default)]
    app_pages: Vec<(String, Element)>,
    /// Fields rendered BELOW Language + Theme on the General tab (the news checkbox is one).
    #[props(default)]
    general_extra: Option<Element>,
    /// Content rendered BELOW the About block.
    #[props(default)]
    about_extra: Option<Element>,
) -> Element {
    let mut active = use_signal(|| if initial_tab.is_empty() { GENERAL.to_owned() } else { initial_tab.clone() });
    // Tabs visited so far: a page in `app_pages` mounts the first time its tab is shown and then stays mounted.
    let mut visited = use_signal(|| vec![if initial_tab.is_empty() { GENERAL.to_owned() } else { initial_tab.clone() }]);
    let mut goto = move |key: String| {
        active.set(key.clone());
        if !visited.peek().contains(&key) {
            visited.write().push(key.clone());
        }
        if let Some(h) = on_tab {
            h.call(key);
        }
    };

    // The keyboard belongs to the page while it is open (see `KEYS_JS`).
    let view_id = use_hook(|| format!("stg-view-{}", NEXT_VIEW.fetch_add(1, Ordering::Relaxed)));
    let bridge = use_hook(|| Rc::new(Cell::new(Option::<Eval>::None)));
    let leave = use_callback(move |_: ()| on_close.call(()));
    {
        let (bridge, view_id) = (bridge.clone(), view_id.clone());
        use_future(move || {
            let (bridge, view_id) = (bridge.clone(), view_id.clone());
            async move {
                let mut ev = document::eval(&format!("const VIEW_ID = {}; {KEYS_JS}", serde_json::to_string(&view_id).unwrap_or_default()));
                bridge.set(Some(ev));
                while let Ok(msg) = ev.recv::<String>().await {
                    if msg == "close" {
                        leave.call(());
                    }
                }
            }
        });
    }
    {
        let bridge = bridge.clone();
        use_drop(move || {
            if let Some(ev) = bridge.get() {
                let _ = ev.send("dispose");
            }
        });
    }

    // While the page is open <html> carries `.stg-open`, which ui-app.css uses to hide the footer
    // bars and make the rest of the header inert. Removed again when the page unmounts.
    use_hook(|| {
        let _ = document::eval("document.documentElement.classList.add('stg-open');");
    });
    use_drop(|| {
        let _ = document::eval("document.documentElement.classList.remove('stg-open');");
    });

    let t = |key: &str| i18n::t(&lang, key);
    let ttl = t("ttlStg");
    let tip_back = t("tipStgBack");
    let tab_general = t("tabStgGeneral");
    let tab_about = t("tabStgAbout");
    let lbl_lang = t("lblStgGeneralLang");
    let lbl_theme = t("lblStgGeneralTheme");
    let btn_dark = t("btnStgGeneralThemeDark");
    let btn_light = t("btnStgGeneralThemeLight");
    let about_desc = t("msgStgAboutDesc");

    let tab_class = |key: &str| if active() == key { "tab active" } else { "tab" };
    let is_app_tab = app_tabs.iter().any(|t| t.key == active());
    let opt_class = |key: &str| if theme == key { "opt-btn active" } else { "opt-btn" };

    rsx! {
        div { id: "{view_id}", class: "stg-view",
            div { class: "stg-rail pnl",
                div { class: "stg-head",
                    button {
                        class: "btn icon subtle",
                        title: "{tip_back}",
                        "aria-label": "{tip_back}",
                        onclick: move |_| on_close.call(()),
                        Icon { width: 14, height: 14, icon: LdArrowLeft }
                    }
                    span { class: "dlg-title", "{ttl}" }
                }
                div { class: "tabs vertical",
                    button {
                        class: tab_class(GENERAL),
                        onclick: move |_| {
                            goto(GENERAL.to_owned());
                        },
                        Icon { width: 14, height: 14, icon: LdSun }
                        "{tab_general}"
                    }
                    for tab in app_tabs.iter().cloned() {
                        {
                            let key = tab.key.clone();
                            rsx! {
                                button {
                                    key: "{tab.key}",
                                    class: tab_class(&tab.key),
                                    onclick: move |_| {
                                        goto(key.clone());
                                    },
                                    {tab.icon.clone()}
                                    "{tab.label}"
                                }
                            }
                        }
                    }
                    button {
                        class: tab_class(ABOUT),
                        onclick: move |_| {
                            goto(ABOUT.to_owned());
                        },
                        Icon { width: 14, height: 14, icon: LdScrollText }
                        "{tab_about}"
                    }
                }
            }
            div { class: "stg-body dlg-form",
                div { class: "stg-page", hidden: active() != GENERAL,
                    div { class: "dlg-field",
                        label { class: "dlg-field-label", "{lbl_lang}" }
                        select {
                            class: "select",
                            value: "{lang}",
                            onchange: move |e| on_set_lang.call(e.value()),
                            for l in LANGUAGES.iter() {
                                option { key: "{l.key}", value: "{l.key}", selected: l.key == lang, "{l.label}" }
                            }
                        }
                    }
                    div { class: "dlg-field divider",
                        label { class: "dlg-field-label", "{lbl_theme}" }
                        div { class: "opt-btns",
                            button {
                                class: opt_class("dark"),
                                onclick: move |_| on_set_theme.call("dark".to_owned()),
                                Icon { width: 14, height: 14, icon: LdMoon }
                                span { "{btn_dark}" }
                            }
                            button {
                                class: opt_class("light"),
                                onclick: move |_| on_set_theme.call("light".to_owned()),
                                Icon { width: 14, height: 14, icon: LdSun }
                                span { "{btn_light}" }
                            }
                        }
                    }
                    if let Some(extra) = general_extra {
                        {extra}
                    }
                }
                for (key, page) in app_pages.iter() {
                    if visited().contains(key) {
                        div { key: "{key}", class: "stg-page stg-page-app", hidden: active() != *key, {page.clone()} }
                    }
                }
                if is_app_tab && !app_pages.iter().any(|(k, _)| *k == active()) {
                    div { class: "stg-page stg-page-app",
                        if let Some(page) = app_page {
                            {page}
                        }
                    }
                }
                div { class: "stg-page", hidden: active() != ABOUT,
                    div { class: "dlg-about",
                        img { class: "dlg-about-logo", src: "{*LOGO}", alt: "yaiol" }
                        div { class: "dlg-about-text",
                            div { class: "dlg-about-id", "{app_name} " b { "v{app_version}" } " by yaiol" }
                            div { class: "dlg-about-desc", "{about_desc}" }
                        }
                    }
                    if let Some(extra) = about_extra {
                        {extra}
                    }
                }
            }
        }
    }
}
