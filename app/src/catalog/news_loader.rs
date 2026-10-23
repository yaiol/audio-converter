// twin: ui-news-loader.jsx jsx@b8052d389adf rsx@9ed4a20e0132
//! NewsLoader — twin of `ui-news-loader.jsx`: the one-line wiring that makes an app show yaiol news.
//! Once on launch it asks the news module what this app should show; if there is anything it renders the
//! shared `NewsDialog`, and when the reader closes it, remembers the ids they asked not to see again.
//! Silent on every failure. Always render it: the news module decides what is shown. A new version comes first
//! and is shown whatever the user’s "Show news from yaiol" setting says; with the setting off no announcement is
//! shown or requested. Pass `app_version` (the running release version) to have the new-version check; left empty
//! there is none, as for an app that has not adopted it.
//!
//!   NewsLoader { app_id: APP_ID, app_version: APP_VERSION, lang: lang() }
//!
//! The JSX twin asks its app’s local server over HTTP; here the module is called directly, off the UI
//! thread. The dialog reads its four words from the app's own `crate::i18n`.
//! Differs: behaviour none. Form only: it calls the news module directly, where the JSX asks its app's local server
//! over HTTP.

use super::news;
use super::news_dialog::{NewsDialog, NewsItem};
use dioxus::prelude::*;

#[component]
pub fn NewsLoader(app_id: String, lang: String, #[props(default)] app_version: String) -> Element {
    let mut items = use_signal(|| Option::<Vec<NewsItem>>::None);

    // Once, on launch.
    {
        let (app_id, app_version, lang) = (app_id.clone(), app_version.clone(), lang.clone());
        use_future(move || {
            let (app_id, app_version, lang) = (app_id.clone(), app_version.clone(), lang.clone());
            async move {
                // The user’s own date, from the page: the standard library has no local time zone.
                let today = document::eval("const d = new Date(); dioxus.send(d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0'));")
                    .recv::<String>()
                    .await
                    .unwrap_or_default();
                let found = tokio::task::spawn_blocking(move || news::current(&app_id, &app_version, &lang, &today)).await.unwrap_or_default();
                if !found.is_empty() {
                    items.set(Some(found));
                }
            }
        });
    }

    let Some(list) = items() else { return rsx! {} };
    rsx! {
        NewsDialog {
            items: list,
            lang,
            on_close: move |hide_ids: Vec<String>| {
                if !hide_ids.is_empty() {
                    spawn(async move {
                        let _ = tokio::task::spawn_blocking(move || news::dismiss(&hide_ids)).await;
                    });
                }
                items.set(None);
            },
        }
    }
}
