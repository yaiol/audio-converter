//! AppHeader — twin of `ui-header.jsx`: app name, version badge, children, then the 2px
//! gradient identity separator. Not ported yet: the optional `subBar`.

use dioxus::prelude::*;

#[component]
pub fn AppHeader(app_name: String, app_version: String, children: Element) -> Element {
    rsx! {
        div { class: "barh-header",
            span { class: "barh-app-name", "{app_name}" }
            span { class: "barh-app-version", "v{app_version}" }
            {children}
        }
        div { class: "barh-gradient" }
    }
}
