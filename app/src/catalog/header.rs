// twin: ui-header.jsx jsx@ee3a37c52cdd rsx@69523206cd68
//! AppHeader — twin of `ui-header.jsx`: app name, version badge, children, then the 2px gradient identity
//! separator, then an OPTIONAL sub-bar (a tag / filter strip) below it - so the order is always
//! "header buttons -> separator -> optional sub-bar".
//! Differs: none.

use dioxus::prelude::*;

#[component]
pub fn AppHeader(
    app_name: String,
    app_version: String,
    /// The strip below the separator; omit when there is none.
    #[props(default)]
    sub_bar: Option<Element>,
    children: Element,
) -> Element {
    rsx! {
        div { class: "barh-header",
            span { class: "barh-app-name", "{app_name}" }
            span { class: "barh-app-version", "v{app_version}" }
            {children}
        }
        div { class: "barh-gradient" }
        if let Some(bar) = sub_bar {
            div { class: "barh-subbar", {bar} }
        }
    }
}
