//! UpdateBanner — twin of `ui-update-banner.jsx`: "a new version is available", notify-only and
//! dismissible. Same classes and element order, so the shared `.update-banner*` CSS skins it as is.
//! The app owns the update state; this component only renders it and reports Skip / Dismiss.

use crate::{i18n, update::site_url};
use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdX};

#[component]
pub fn UpdateBanner(
    version: String,
    app_id: String,
    lang: String,
    on_skip: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let t = |key: &str| i18n::t(&lang, key);
    let label = t("lblUpdateAvailable");
    let whats_new = t("lnkUpdateWhatsNew");
    let download = t("btnUpdateDownload");
    let skip = t("lnkUpdateSkip");
    let dismiss = t("tipUpdateDismiss");

    let releases_url = site_url(&app_id, &lang, "help/releases");
    let download_url = site_url(&app_id, &lang, "download");

    rsx! {
        div { class: "update-banner",
            div { class: "update-banner-row",
                span { class: "update-banner-label", "{label} v{version}" }
                button {
                    class: "update-banner-link",
                    onclick: move |_| {
                        let _ = open::that(&releases_url);
                    },
                    "{whats_new}"
                }
                span { class: "update-banner-spacer" }
                button {
                    class: "update-banner-download",
                    onclick: move |_| {
                        let _ = open::that(&download_url);
                    },
                    "{download}"
                }
                button { class: "update-banner-link skip", onclick: move |_| on_skip.call(()), "{skip}" }
                button {
                    class: "update-banner-dismiss",
                    title: "{dismiss}",
                    "aria-label": "{dismiss}",
                    onclick: move |_| on_close.call(()),
                    Icon { width: 16, height: 16, icon: LdX }
                }
            }
        }
    }
}
