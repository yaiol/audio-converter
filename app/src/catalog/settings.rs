//! SettingsView — twin of `ui-settings.jsx`: the full-window settings page. Same classes and
//! element structure (rail + page, `.tabs.vertical`, `.dlg-field`, `.opt-btns`, `.dlg-about`).
//!
//! Ported: frame, Display (Language + Theme), About, app-supplied `tabs` (+ the page of the active
//! one), the `stg-open` class on <html>.
//! Pending: `displayExtra` / `aboutExtra`, the keyboard capture (Esc to leave, focus trap) - it
//! relies on document-level key listeners.

use crate::i18n::{self, LANGUAGES};
use dioxus::prelude::*;
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

/// A tab the APP adds between Display and About. `key` is the app's own identifier for it.
#[derive(Clone, PartialEq)]
pub struct AppTab {
    pub key: String,
    pub label: String,
    pub icon: Element,
}

const DISPLAY: &str = "display";
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
    /// Tabs the app adds, shown between Display and About.
    #[props(default)]
    app_tabs: Vec<AppTab>,
    /// The tab to open on: `display`, `about`, or an app tab's key. Empty = Display.
    #[props(default)]
    initial_tab: String,
    /// Told the key of the tab that became active (and of the one it opened on), so the app can
    /// supply `app_page` for it.
    #[props(default)]
    on_tab: Option<EventHandler<String>>,
    /// The page of the active APP tab. Ignored while a built-in tab is active.
    #[props(default)]
    app_page: Option<Element>,
) -> Element {
    let mut active = use_signal(|| if initial_tab.is_empty() { DISPLAY.to_owned() } else { initial_tab.clone() });

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
    let tab_display = t("tabStgDisplay");
    let tab_about = t("tabStgAbout");
    let lbl_lang = t("lblStgDisplayLang");
    let lbl_theme = t("lblStgDisplayTheme");
    let btn_dark = t("btnStgDisplayThemeDark");
    let btn_light = t("btnStgDisplayThemeLight");
    let about_desc = t("msgStgAboutDesc");

    let tab_class = |key: &str| if active() == key { "tab active" } else { "tab" };
    let is_app_tab = app_tabs.iter().any(|t| t.key == active());
    let opt_class = |key: &str| if theme == key { "opt-btn active" } else { "opt-btn" };

    rsx! {
        div { class: "stg-view",
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
                        class: tab_class(DISPLAY),
                        onclick: move |_| {
                            active.set(DISPLAY.to_owned());
                            if let Some(h) = on_tab { h.call(DISPLAY.to_owned()); }
                        },
                        Icon { width: 14, height: 14, icon: LdSun }
                        "{tab_display}"
                    }
                    for tab in app_tabs.iter().cloned() {
                        {
                            let key = tab.key.clone();
                            rsx! {
                                button {
                                    key: "{tab.key}",
                                    class: tab_class(&tab.key),
                                    onclick: move |_| {
                                        active.set(key.clone());
                                        if let Some(h) = on_tab { h.call(key.clone()); }
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
                            active.set(ABOUT.to_owned());
                            if let Some(h) = on_tab { h.call(ABOUT.to_owned()); }
                        },
                        Icon { width: 14, height: 14, icon: LdScrollText }
                        "{tab_about}"
                    }
                }
            }
            div { class: "stg-body dlg-form",
                div { class: "stg-page", hidden: active() != DISPLAY,
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
                }
                if is_app_tab {
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
                }
            }
        }
    }
}
