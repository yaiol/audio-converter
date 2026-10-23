// twin: ui-news-setting.jsx jsx@f17d1e96328f rsx@5198e4425b0d
//! NewsSetting — twin of `ui-news-setting.jsx`: the "Show news from yaiol" checkbox for a Settings page.
//! It is the SAME per-machine switch in every yaiol app (one small file in the shared `yaiol` app-data
//! folder), so unticking it in one app silences news in all of them. Same classes and element order as the
//! JSX, so it sits in `SettingsView`’s `general_extra` slot like any other General field.
//!
//!   let mut news_on = use_signal(news::read_state().enabled);       // or use_hook(...)
//!   SettingsView { general_extra: rsx! { NewsSetting { enabled: news_on(), label: t("lblStgGeneralNews"),
//!       on_change: move |on| { news::set_enabled(on); news_on.set(on); } } }, .. }
//!
//! Controlled: the app owns the value and persists it (`news::set_enabled`).
//! Differs: none.

use dioxus::prelude::*;

#[component]
pub fn NewsSetting(enabled: bool, label: String, on_change: EventHandler<bool>) -> Element {
    rsx! {
        div { class: "dlg-field divider",
            label { class: "check-label",
                input {
                    r#type: "checkbox",
                    class: "check-box",
                    checked: enabled,
                    onchange: move |e| on_change.call(e.checked()),
                }
                "{label}"
            }
        }
    }
}
