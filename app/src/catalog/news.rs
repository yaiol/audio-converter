// twin: news.mjs jsx@0ad704456c67 rsx@7e8d4886b3b3
//! News — twin of `news.mjs`: what a yaiol app may show its user as news, and the per-MACHINE state that
//! says whether to show any. Same feed, same rules, same state file as the electron apps.
//!
//! - **The feed**: `https://apps.yaiol.com/news.json`, `{ version, announcements: [...] }`, fetched once on launch. An
//!   announcement carries its own `version` (the edition of the format it is written in): one written in a later
//!   edition than this reader knows is skipped. It is shown only to the apps it names.
//! - **The state**: ONE small file in the shared `yaiol` app-data folder (`news-state.json`), read by every
//!   yaiol app on the machine: the on/off switch and the ids the user dismissed. A missing or unreadable
//!   file means "news on, nothing dismissed"; it is written atomically (temp file + rename) because several
//!   apps may save at once.
//! - **Absolute off**: when the switch is off no announcement is shown and, because the state is read FIRST,
//!   none is requested. Every failure is silent: offline, bad file, bad feed => no news.
//! - **A new version** comes first and is NOT governed by that switch (it is about announcements only): the beacon
//!   `p/<id>/latest.json` says which version is out and, only when it is newer than the one running, the notes
//!   file of the reader's language (`p/<id>/<lang>.json`: own language, base language, English) gives the release
//!   blocks newer than the running version make up ONE card. Its id names the latest version, so "don't show
//!   again" silences the update until a newer version comes out.
//!
//! BLOCKING (a network round trip, file I/O): call `current` through `tokio::task::spawn_blocking`, never
//! straight from a component. `today` is the user's local date as `YYYY-MM-DD`; the caller asks the page
//! (`new Date()`), because the standard library has no local time zone. Start and end are whole days, so
//! comparing dates as text is exact.
//! Differs: behaviour none. Form only: it is a module called directly, where the JSX side is the same rules behind
//! HTTP routes on the app's local server; the user's local date is passed in (the standard library has no time zone).

use super::news_dialog::{NewsItem, NewsLang};
use serde_json::Value;
use std::collections::BTreeMap;
use std::{fs, path::PathBuf, time::Duration};

const FEED_URL: &str = "https://apps.yaiol.com/news.json";
const FETCH_TIMEOUT_SECS: u64 = 8;
const SITE_APPS: &str = "https://apps.yaiol.com";

/// The edition of the announcement format this reader understands. A missing number reads as 1.
const READER_VERSION: u64 = 1;

/// The per-machine news state.
#[derive(Clone, PartialEq, Debug)]
pub struct NewsState {
    pub enabled: bool,
    pub dismissed: Vec<String>,
}

impl Default for NewsState {
    fn default() -> Self {
        Self { enabled: true, dismissed: Vec::new() }
    }
}

fn state_file() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("yaiol").join("news-state.json"))
}

/// The state out of its file's text. Anything but a clean object reads as the default.
fn parse_state(text: &str) -> NewsState {
    let Ok(raw) = serde_json::from_str::<Value>(text) else { return NewsState::default() };
    NewsState {
        // only an explicit `false` turns news off
        enabled: raw.get("enabled") != Some(&Value::Bool(false)),
        dismissed: raw
            .get("dismissed")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
    }
}

pub fn read_state() -> NewsState {
    state_file().and_then(|f| fs::read_to_string(f).ok()).map(|t| parse_state(&t)).unwrap_or_default()
}

fn write_state(state: &NewsState) {
    let Some(file) = state_file() else { return };
    let body = serde_json::json!({ "enabled": state.enabled, "dismissed": state.dismissed });
    let tmp = file.with_extension(format!("{}.tmp", std::process::id()));
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if fs::write(&tmp, serde_json::to_string_pretty(&body).unwrap_or_default()).is_ok() {
        let _ = fs::rename(&tmp, &file);
    }
}

/// The "Show news from yaiol" setting.
pub fn set_enabled(enabled: bool) {
    let mut state = read_state();
    state.enabled = enabled;
    write_state(&state);
}

/// "Don't show again": remember these ids on this machine, for every yaiol app.
pub fn dismiss(ids: &[String]) {
    let mut state = read_state();
    let fresh: Vec<String> = ids.iter().filter(|id| !id.is_empty() && !state.dismissed.contains(id)).cloned().collect();
    if fresh.is_empty() {
        return;
    }
    state.dismissed.extend(fresh);
    write_state(&state);
}

fn load_feed() -> Option<Value> {
    if std::env::var_os("YAIOL_DEV").is_some() {
        return None;
    }
    fetch_json(FEED_URL)
}

fn fetch_json(url: &str) -> Option<Value> {
    let mut resp = ureq::get(url)
        .header("Cache-Control", "no-cache")
        .config()
        .timeout_global(Some(Duration::from_secs(FETCH_TIMEOUT_SECS)))
        .build()
        .call()
        .ok()?;
    serde_json::from_str(&resp.body_mut().read_to_string().ok()?).ok()
}

/// "Who sees it": `only` these apps, or everyone `all-but` these. Anything else is seen by nobody.
fn is_targeted(target: Option<&Value>, app_id: &str) -> bool {
    let Some(target) = target else { return false };
    let apps: Vec<&str> = target
        .get("apps")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    match target.get("mode").and_then(Value::as_str) {
        Some("only") => apps.contains(&app_id),
        Some("all-but") => !apps.contains(&app_id),
        _ => false,
    }
}

/// An announcement written in a later edition than this reader understands is skipped.
fn is_readable(version: Option<&Value>) -> bool {
    version.and_then(Value::as_u64).is_none_or(|v| v <= READER_VERSION)
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

/// A start date that has not come yet. A malformed one never blocks.
fn is_not_started(start: Option<&str>, today: &str) -> bool {
    matches!(start, Some(s) if is_date(s) && s > today)
}

/// An end date that has passed (the whole end day still counts). A malformed one never expires.
fn is_expired(end: Option<&str>, today: &str) -> bool {
    matches!(end, Some(e) if is_date(e) && e < today)
}

/// The `lang` object of an announcement: `{ language: { title, subtitle, text, image } }`. A block that is not an object is dropped.
fn lang_map(v: Option<&Value>) -> BTreeMap<String, NewsLang> {
    let field = |b: &Value, k: &str| b.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    v.and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter(|(_, b)| b.is_object())
                .map(|(k, b)| (k.clone(), NewsLang { title: field(b, "title"), subtitle: field(b, "subtitle"), text: field(b, "text"), image: field(b, "image") }))
                .collect()
        })
        .unwrap_or_default()
}

/// The announcements of a feed that THIS app, today, with this state, should show.
fn select(feed: &Value, state: &NewsState, app_id: &str, today: &str) -> Vec<NewsItem> {
    let Some(list) = feed.get("announcements").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(|a| {
            let id = a.get("id")?.as_str()?;
            let lang = lang_map(a.get("lang"));
            let keep = a.get("draft") != Some(&Value::Bool(true))
                && is_readable(a.get("version"))
                && !state.dismissed.iter().any(|d| d == id)
                && !is_not_started(a.get("start").and_then(Value::as_str), today)
                && !is_expired(a.get("end").and_then(Value::as_str), today)
                && is_targeted(a.get("target"), app_id)
                && lang.values().any(|l| !l.title.is_empty());
            keep.then(|| NewsItem {
                id: id.to_owned(),
                lang,
                link: a.get("link").and_then(Value::as_str).map(str::to_owned),
                ..Default::default()
            })
        })
        .collect()
}

/// The card of an update: ONE card for `latest`, with the download page as its link. Its text is the notes of the
/// release blocks of `releases` newer than `app_version`: a single one is just its notes, several are listed in file order
/// (newest first) each under its own bold line. No notes at all still gives the card, with an empty text.
fn update_cards(app_id: &str, app_version: &str, latest: &str, releases: Option<&Value>, code: &str) -> Vec<NewsItem> {
    let newer: Vec<(&str, &str, &str)> = releases
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|r| Some((r.get("version")?.as_str()?, r.get("date").and_then(Value::as_str).unwrap_or(""), r.get("text")?.as_str()?)))
                .filter(|(v, _, _)| engine::compare_versions(v, app_version).is_gt())
                .collect()
        })
        .unwrap_or_default();
    let text = match newer.as_slice() {
        [] => String::new(),
        [(_, _, t)] => (*t).to_owned(),
        many => many
            .iter()
            .map(|(v, d, t)| if d.is_empty() { format!("**v{v}**\n\n{t}") } else { format!("**v{v} — {d}**\n\n{t}") })
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    vec![NewsItem {
        id: format!("update:{app_id}:{latest}"),
        kind: "update".into(),
        version: latest.to_owned(),
        text,
        link: Some(format!("{SITE_APPS}/{code}/p/{app_id}/download/")),
        ..Default::default()
    }]
}

/// A newer version of this app, as one card (none when up to date, dismissed, or offline). Not governed by the news switch.
fn current_update(app_id: &str, app_version: &str, lang: &str, state: &NewsState) -> Vec<NewsItem> {
    if std::env::var_os("YAIOL_DEV").is_some() || app_version.is_empty() {
        return Vec::new();
    }
    let base = format!("{SITE_APPS}/p/{app_id}");
    let Some(latest) = fetch_json(&format!("{base}/latest.json")).and_then(|b| b.get("version")?.as_str().map(str::to_owned)) else { return Vec::new() };
    if engine::compare_versions(&latest, app_version).is_le() || state.dismissed.contains(&format!("update:{app_id}:{latest}")) {
        return Vec::new();
    }
    let code = lang.replace('_', "-");
    let primary = code.split('-').next().unwrap_or("").to_owned();
    let mut releases = None;
    for k in [code.as_str(), primary.as_str(), "en"] {
        if let Some(list) = fetch_json(&format!("{base}/{k}.json")).and_then(|b| b.get("releases").cloned()).filter(Value::is_array) {
            releases = Some(list);
            break;
        }
    }
    update_cards(app_id, app_version, &latest, releases.as_ref(), &code)
}

/// What to show now: first a new version (when `app_version` is given and a newer one is out), then the feed’s
/// announcements that are live, targeted at `app_id` and not yet dismissed - none of them, and no request for
/// them, when the news switch is off. Silent on every failure.
pub fn current(app_id: &str, app_version: &str, lang: &str, today: &str) -> Vec<NewsItem> {
    let state = read_state();
    let mut items = current_update(app_id, app_version, lang, &state);
    if state.enabled
        && let Some(feed) = load_feed()
    {
        items.extend(select(&feed, &state, app_id, today));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed() -> Value {
        serde_json::json!({ "announcements": [
            { "id": "a", "lang": { "en": { "title": "A" } }, "target": { "mode": "all-but", "apps": ["markzen"] } },
            { "id": "b", "lang": { "en": { "title": "B" } }, "target": { "mode": "only", "apps": ["markzen"] } },
            { "id": "old", "lang": { "en": { "title": "Old" } }, "end": "2026-01-31", "target": { "mode": "all-but", "apps": [] } },
            { "id": "soon", "lang": { "en": { "title": "Soon" } }, "start": "2026-12-01", "target": { "mode": "all-but", "apps": [] } },
            { "id": "notitle", "lang": { "en": { "title": "" } }, "target": { "mode": "all-but", "apps": [] } },
            { "id": "notarget", "lang": { "en": { "title": "N" } } },
            { "lang": { "en": { "title": "no id" } }, "target": { "mode": "all-but", "apps": [] } },
        ]})
    }

    fn ids(items: &[NewsItem]) -> Vec<&str> {
        items.iter().map(|i| i.id.as_str()).collect()
    }

    #[test]
    fn targeting_dates_and_titles() {
        let s = NewsState::default();
        assert_eq!(ids(&select(&feed(), &s, "lrc-editor", "2026-10-18")), ["a"]);
        assert_eq!(ids(&select(&feed(), &s, "markzen", "2026-10-18")), ["b"]);
        // the whole end day still counts, and a start day shows from its first moment
        assert_eq!(ids(&select(&feed(), &s, "lrc-editor", "2026-01-31")), ["a", "old"]);
        assert_eq!(ids(&select(&feed(), &s, "lrc-editor", "2026-12-01")), ["a", "soon"]);
    }

    #[test]
    fn the_picture_and_the_language_blocks_come_through() {
        let feed = serde_json::json!({ "announcements": [
            { "id": "p", "lang": { "en": { "title": "P", "text": "T", "image": "p.en.png" }, "fr": { "title": "Pf", "subtitle": "S" }, "xx": "not a block" },
              "target": { "mode": "all-but", "apps": [] } },
        ]});
        let items = select(&feed, &NewsState::default(), "lrc-editor", "2026-10-18");
        assert_eq!(items[0].lang["en"].image, "p.en.png");
        assert_eq!(items[0].lang["fr"].subtitle, "S");
        assert_eq!(items[0].lang["en"].text, "T");
        assert!(!items[0].lang.contains_key("xx"));
    }

    #[test]
    fn a_draft_is_never_shown() {
        let feed = serde_json::json!({ "announcements": [
            { "id": "live", "lang": { "en": { "title": "L" } }, "target": { "mode": "all-but", "apps": [] } },
            { "id": "wip", "draft": true, "lang": { "en": { "title": "W" } }, "target": { "mode": "all-but", "apps": [] } },
            { "id": "notdraft", "draft": false, "lang": { "en": { "title": "N" } }, "target": { "mode": "all-but", "apps": [] } },
        ]});
        assert_eq!(ids(&select(&feed, &NewsState::default(), "lrc-editor", "2026-10-18")), ["live", "notdraft"]);
    }

    #[test]
    fn dismissed_ids_stay_hidden() {
        let s = NewsState { enabled: true, dismissed: vec!["a".into()] };
        assert!(select(&feed(), &s, "lrc-editor", "2026-10-18").is_empty());
    }

    #[test]
    fn state_file_is_read_tolerantly() {
        assert_eq!(parse_state("{}"), NewsState::default());
        assert_eq!(parse_state("not json"), NewsState::default());
        assert!(!parse_state(r#"{"enabled":false}"#).enabled);
        assert!(parse_state(r#"{"enabled":0}"#).enabled);
        assert_eq!(parse_state(r#"{"dismissed":["x",1,"y"]}"#).dismissed, ["x", "y"]);
    }

    #[test]
    fn malformed_dates_never_block_or_expire() {
        assert!(!is_not_started(Some("tomorrow"), "2026-10-18"));
        assert!(!is_expired(Some("2026-1-1"), "2026-10-18"));
        assert!(!is_expired(None, "2026-10-18"));
    }

    /// An announcement with these extra top-level fields, for "lrc-editor".
    fn shown(extra: Value) -> bool {
        let mut a = serde_json::json!({ "id": "x", "lang": { "en": { "title": "X" } }, "target": { "mode": "all-but", "apps": [] } });
        for (k, v) in extra.as_object().unwrap() {
            a[k] = v.clone();
        }
        !select(&serde_json::json!({ "announcements": [a] }), &NewsState::default(), "lrc-editor", "2026-10-18").is_empty()
    }

    #[test]
    fn an_update_is_one_card_with_the_notes_of_every_newer_release() {
        let releases = serde_json::json!([
            { "version": "1.0.8", "date": "2026-09-28", "text": "- eight" },
            { "version": "1.0.7", "date": "2026-09-27", "text": "- seven" },
            { "version": "1.0.6", "date": "2026-09-20", "text": "- six" },
        ]);
        let cards = update_cards("lrc-editor", "1.0.6", "1.0.8", Some(&releases), "fr");
        assert_eq!(ids(&cards), ["update:lrc-editor:1.0.8"]);
        assert_eq!(cards[0].kind, "update");
        assert_eq!(cards[0].text, "**v1.0.8 — 2026-09-28**\n\n- eight\n\n**v1.0.7 — 2026-09-27**\n\n- seven");
        assert_eq!(cards[0].link.as_deref(), Some("https://apps.yaiol.com/fr/p/lrc-editor/download/"));
        // a single newer release is just its notes, with no heading
        let one = update_cards("lrc-editor", "1.0.7", "1.0.8", Some(&releases), "en");
        assert_eq!(one[0].text, "- eight");
    }

    #[test]
    fn an_update_without_notes_is_still_one_card() {
        let cards = update_cards("lrc-editor", "1.0.8", "1.0.9", None, "en");
        assert_eq!(ids(&cards), ["update:lrc-editor:1.0.9"]);
        assert_eq!(cards[0].version, "1.0.9");
        assert!(cards[0].text.is_empty());
    }

    #[test]
    fn an_announcement_of_a_later_edition_is_skipped() {
        assert!(shown(serde_json::json!({})));
        assert!(shown(serde_json::json!({ "version": 1 })));
        assert!(!shown(serde_json::json!({ "version": 2 })));
    }
}
