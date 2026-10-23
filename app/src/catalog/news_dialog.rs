// twin: ui-news-dialog.jsx jsx@fea84a9985f2 rsx@92bc33fb136d
//! NewsDialog — twin of `ui-news-dialog.jsx`: the ONE pop-up that shows yaiol news, in every app that
//! shows news. Controlled and fetch-free:
//!
//!   `items`  - the announcements, or ONE update card (`kind == "update"`: the latest version and the notes of every release newer
//!              than the one running, see `news.rs`; title = `lblUpdateAvailable` + the version, text = the notes, button =
//!              `btnUpdateDownload`). Several turn as a carousel: by itself (about reading time, 5-15 s each)
//!              until the reader clicks a number / arrow (or ticks the box), and the dialog keeps one size
//!              throughout. `lang` is `{ language: { title, subtitle, text } }`, resolved HERE field by field by
//!              language -> base language -> `en`, the picture included. An `image` is an https:// address,
//!              a data:image URL or a bare file name (read from apps.yaiol.com/news/); anything else is ignored.
//!   `lang`   - the app's UI language (`pt_BR` or `pt-BR`)
//!   words    - the four the dialog needs (`lblNewsHideForever`, `btnNewsOpenLink`, `btnNewsClose`, `tipNewsDismiss`) are
//!              read from the app's own `crate::i18n`, as the settings page and the update banner do
//!   `on_close` - fired by the X, Esc, a click outside and Close; receives the ids whose "don't show
//!              again" box was ticked (the box belongs to the announcement being shown)
//!
//! An item whose title cannot be resolved is skipped; with none left the dialog renders nothing. Same
//! classes and element order, so the `.dlp*` rules in `ui-app.css` skin it. Links open in the system
//! browser. Needs the app skeleton's `open` and `base64` crates (the logo is embedded the same way the
//! settings page's is).
//! Differs: behaviour none. Form only: the four words come from the app's own `crate::i18n` (like the settings page
//! and the update banner) instead of a passed `t`; the logo is embedded as a data URL.

use super::news_text::NewsText;
use crate::i18n;
use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdChevronLeft, LdChevronRight, LdX},
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

// Embedded rather than read from disk: a `cargo build` executable must carry its own logo.
static LOGO: LazyLock<String> = LazyLock::new(|| {
    use base64::{Engine, engine::general_purpose::STANDARD};
    format!("data:image/svg+xml;base64,{}", STANDARD.encode(include_bytes!("../../assets/yaiol-logo.svg")))
});

/// One language's words of an announcement.
#[derive(Clone, PartialEq, Default, Deserialize)]
pub struct NewsLang {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub text: String,
    /// The picture, a file name or an address: picked per language like the words (own language, base language, English).
    #[serde(default)]
    pub image: String,
}

/// One announcement, in the shape of the news feed: the words and the picture per language, and what is the same in every language.
#[derive(Clone, PartialEq, Default, Deserialize)]
pub struct NewsItem {
    pub id: String,
    #[serde(default)]
    pub lang: BTreeMap<String, NewsLang>,
    #[serde(default)]
    pub link: Option<String>,
    /// "update" for a card of a new version; empty for an announcement.
    #[serde(default)]
    pub kind: String,
    /// An update card's version and its release notes (a small markdown subset, as an announcement's text).
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub text: String,
}

/// A card's title: an update's is the app's "new version" label and the version; an announcement's comes from its languages.
fn card_title(it: &NewsItem, lang: &str, update_label: &str) -> String {
    if it.kind == "update" { format!("{update_label} v{}", it.version) } else { pick_news_field(&it.lang, lang, |l| &l.title) }
}

fn card_text(it: &NewsItem, lang: &str) -> String {
    if it.kind == "update" { it.text.clone() } else { pick_news_field(&it.lang, lang, |l| &l.text) }
}

/// One field of an announcement in the reader's language: that language's own block, else its base language's,
/// else English's - per FIELD, as the JSX's `pickNewsField`.
pub fn pick_news_field(langs: &BTreeMap<String, NewsLang>, lang: &str, field: impl Fn(&NewsLang) -> &str) -> String {
    let code = lang.replace('_', "-");
    let base = code.split('-').next().unwrap_or("");
    [code.as_str(), base, "en"]
        .iter()
        .filter_map(|k| langs.get(*k))
        .map(&field)
        .find(|v| !v.is_empty())
        .map(str::to_owned)
        .unwrap_or_default()
}

const NEWS_IMAGES: &str = "https://apps.yaiol.com/news/";

/// The address an `image` value stands for, or an empty string when it is not one we show (as the JSX's `newsImageUrl`).
pub fn news_image_url(raw: &str) -> String {
    let v = raw.trim();
    let lower = v.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("data:image/") {
        return v.to_owned();
    }
    let plain = v.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
        && !v.contains("..");
    if plain { format!("{NEWS_IMAGES}{v}") } else { String::new() }
}

/// How long an announcement stays: 4 s plus 1 s per 3 words (about reading speed), kept between 5 s and 15 s.
fn read_ms(it: &NewsItem, lang: &str, update_label: &str) -> u32 {
    let words = [
        card_title(it, lang, update_label),
        pick_news_field(&it.lang, lang, |l| &l.subtitle),
        card_text(it, lang),
    ]
    .join(" ")
        .split_whitespace()
        .count();
    (4000.0 + (words as f64 / 3.0).round() * 1000.0).clamp(5000.0, 15000.0) as u32
}

#[component]
pub fn NewsDialog(
    items: Vec<NewsItem>,
    lang: String,
    on_close: EventHandler<Vec<String>>,
) -> Element {
    let t = |key: &str| i18n::t(&lang, key);
    let (hide_forever, open_link, close_label, dismiss_tip) =
        (t("lblNewsHideForever"), t("btnNewsOpenLink"), t("btnNewsClose"), t("tipNewsDismiss"));
    let (update_label, download_label) = (t("lblUpdateAvailable"), t("btnUpdateDownload"));
    let mut index = use_signal(|| 0usize);
    let mut auto = use_signal(|| true);
    let mut hidden = use_signal(BTreeSet::<String>::new);
    // Images that did not load: the box stays, empty.
    let mut failed = use_signal(BTreeSet::<String>::new);
    // Bumped whenever the turning timer is restarted or cancelled, so a sleeping one knows it is stale.
    let mut epoch = use_signal(|| 0u32);

    let list: Vec<NewsItem> = items.into_iter().filter(|it| !card_title(it, &lang, &update_label).is_empty()).collect();
    let count = list.len();
    // How long each card stays, worked out here so the timer below depends on plain numbers.
    let times: Vec<u32> = list.iter().map(|it| read_ms(it, &lang, &update_label)).collect();
    let current = index().min(count.saturating_sub(1));

    // The carousel turns by itself, each announcement staying as long as it takes to read, until the reader
    // takes over (a click on a number or an arrow, an arrow key, ticking the box) - after that it stays
    // where they put it.
    {
        let times = times.clone();
        use_effect(use_reactive!(|(current, count, times)| {
            epoch += 1;
            let mine = epoch();
            if !auto() || count < 2 {
                return;
            }
            let ms = times[current];
            spawn(async move {
                let _ = document::eval(&format!("await new Promise((r) => setTimeout(r, {ms})); dioxus.send(1);"))
                    .recv::<i32>()
                    .await;
                if epoch() == mine && auto() {
                    index.set((current + 1) % count);
                }
            });
        }));
    }

    if list.is_empty() {
        return rsx! {};
    }
    let item = list[current].clone();
    let hide_ids = move || hidden().into_iter().collect::<Vec<_>>();
    let mut choose = move |i: isize| {
        auto.set(false);
        index.set(i.rem_euclid(count as isize) as usize);
    };
    let item_id = item.id.clone();
    let mut toggle_hidden = move |on: bool| {
        auto.set(false);
        hidden.with_mut(|s| {
            if on {
                s.insert(item_id.clone());
            } else {
                s.remove(&item_id);
            }
        });
    };
    let link_label = if item.kind == "update" { download_label } else { open_link };
    let link = item.link.clone();
    let link_for_click = link.clone();

    rsx! {
        div {
            class: "dl-backdrop",
            style: "z-index: 900",
            // Only a press on the backdrop itself closes: the dialog stops its own presses short of it.
            onmousedown: move |_| on_close.call(hide_ids()),
            div {
                class: "dlp wide",
                onmousedown: move |e| e.stop_propagation(),
                onkeydown: move |e| match e.key() {
                    Key::Escape => on_close.call(hide_ids()),
                    Key::ArrowLeft => choose(current as isize - 1),
                    Key::ArrowRight => choose(current as isize + 1),
                    _ => {}
                },
                button {
                    class: "dl-close",
                    r#type: "button",
                    title: "{dismiss_tip}",
                    "aria-label": "{dismiss_tip}",
                    onclick: move |_| on_close.call(hide_ids()),
                    Icon { width: 16, height: 16, icon: LdX }
                }
                img { class: "dlp-icon", src: "{*LOGO}", alt: "yaiol" }
                // Every announcement is laid out in the SAME grid cell and only the current one is visible,
                // so the dialog is as tall as the tallest one and never resizes when the carousel turns.
                div { class: "dlp-pages",
                    for (i, it) in list.iter().enumerate() {
                        {
                            let (title, text) = (card_title(it, &lang, &update_label), card_text(it, &lang));
                            let subtitle = pick_news_field(&it.lang, &lang, |l| &l.subtitle);
                            let image = news_image_url(&pick_news_field(&it.lang, &lang, |l| &l.image));
                            let image_id = it.id.clone();
                            rsx! {
                                div {
                                    key: "{it.id}",
                                    class: if i == current { "dlp-page" } else { "dlp-page dlp-page-off" },
                                    div { class: "dlp-title", "{title}" }
                                    if !subtitle.is_empty() {
                                        div { class: "dlp-subtitle", "{subtitle}" }
                                    }
                                    if !image.is_empty() {
                                        div { class: "dlp-image",
                                            if !failed().contains(&it.id) {
                                                img {
                                                    src: "{image}",
                                                    alt: "",
                                                    onerror: move |_| {
                                                        failed.with_mut(|s| {
                                                            s.insert(image_id.clone());
                                                        });
                                                    },
                                                }
                                            }
                                        }
                                    }
                                    div { class: "dlp-text", NewsText { text } }
                                }
                            }
                        }
                    }
                }
                if count > 1 {
                    div { class: "dlp-nav",
                        button {
                            r#type: "button",
                            class: "btn icon subtle",
                            onclick: move |_| choose(current as isize - 1),
                            Icon { width: 16, height: 16, icon: LdChevronLeft }
                        }
                        for (i, it) in list.iter().enumerate() {
                            button {
                                key: "{it.id}",
                                r#type: "button",
                                class: if i == current { "chip on" } else { "chip" },
                                onclick: move |_| choose(i as isize),
                                "{i + 1}"
                            }
                        }
                        button {
                            r#type: "button",
                            class: "btn icon subtle",
                            onclick: move |_| choose(current as isize + 1),
                            Icon { width: 16, height: 16, icon: LdChevronRight }
                        }
                    }
                }
                div { class: "dlp-foot",
                    label { class: "check-label",
                        input {
                            r#type: "checkbox",
                            class: "check-box",
                            checked: hidden().contains(&item.id),
                            onchange: move |e| toggle_hidden(e.checked()),
                        }
                        "{hide_forever}"
                    }
                    if let Some(url) = link_for_click {
                        button {
                            r#type: "button",
                            class: "btn primary",
                            autofocus: true,
                            onclick: move |_| {
                                let _ = open::that(&url);
                            },
                            "{link_label}"
                        }
                    }
                    button {
                        r#type: "button",
                        class: "btn subtle",
                        autofocus: link.is_none(),
                        onclick: move |_| on_close.call(hide_ids()),
                        "{close_label}"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(pairs: &[(&str, &str)]) -> BTreeMap<String, NewsLang> {
        pairs.iter().map(|(k, v)| (k.to_string(), NewsLang { title: v.to_string(), ..Default::default() })).collect()
    }

    #[test]
    fn language_falls_back_to_base_then_english() {
        let m = titles(&[("en", "Hello"), ("pt", "Ola"), ("pt-BR", "Oi")]);
        assert_eq!(pick_news_field(&m, "pt_BR", |l| &l.title), "Oi");
        assert_eq!(pick_news_field(&m, "pt-PT", |l| &l.title), "Ola");
        assert_eq!(pick_news_field(&m, "fr", |l| &l.title), "Hello");
        assert_eq!(pick_news_field(&BTreeMap::new(), "fr", |l| &l.title), "");
    }

    #[test]
    fn a_missing_field_falls_back_on_its_own() {
        let mut m = titles(&[("en", "Hello"), ("fr", "Salut")]);
        m.get_mut("en").unwrap().text = "Body".into();
        assert_eq!(pick_news_field(&m, "fr", |l| &l.title), "Salut");
        assert_eq!(pick_news_field(&m, "fr", |l| &l.text), "Body");
    }

    #[test]
    fn image_values_are_https_data_or_a_plain_file_name() {
        assert_eq!(news_image_url("https://x.test/a.png"), "https://x.test/a.png");
        assert_eq!(news_image_url("data:image/svg+xml;base64,AAAA"), "data:image/svg+xml;base64,AAAA");
        assert_eq!(news_image_url(" audio-converter.png "), "https://apps.yaiol.com/news/audio-converter.png");
        assert_eq!(news_image_url("http://x.test/a.png"), "");
        assert_eq!(news_image_url("../secret.png"), "");
        assert_eq!(news_image_url("javascript:alert(1)"), "");
        assert_eq!(news_image_url(""), "");
    }
}
