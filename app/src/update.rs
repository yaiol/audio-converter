//! The update check and the yaiol site URLs — the twin of the shared `update-check.js`.
//!
//! Notify-only, poll-once-on-launch, silent on every failure: the app asks
//! `apps.yaiol.com/p/<id>/latest.json` (a version-only beacon built from the app's `releases.md`),
//! and if a newer version exists that the user has not skipped, the banner offers the What's-new and
//! Download pages. Nothing is downloaded or installed here.

use serde::Deserialize;
use std::time::Duration;

const ENDPOINT: &str = "https://apps.yaiol.com/p";
const SITE: &str = "https://apps.yaiol.com";

/// The localized product page `https://apps.yaiol.com/<lang>/p/<app>/<page>/` (`page` may be empty
/// or nested: `help/releases`). The workspace's language codes use an underscore (`pt_BR`); the web
/// form is a hyphen, so it is converted here, once.
pub fn site_url(app_id: &str, lang: &str, page: &str) -> String {
    let lang = lang.replace('_', "-");
    let path: Vec<&str> = page.split('/').filter(|s| !s.is_empty()).collect();
    if path.is_empty() {
        format!("{SITE}/{lang}/p/{app_id}/")
    } else {
        format!("{SITE}/{lang}/p/{app_id}/{}/", path.join("/"))
    }
}

#[derive(Deserialize)]
struct Beacon {
    version: Option<String>,
}

/// The `version` of a beacon body, or None when it is not JSON or carries no version.
fn beacon_version(body: &str) -> Option<String> {
    serde_json::from_str::<Beacon>(body).ok()?.version.filter(|v| !v.is_empty())
}

/// A newer release the user has not skipped, or None.
///
/// ⚠ CLAUDE: BLOCKING (a network round trip) — call it through `spawn_blocking`, never directly from
/// a component, or the window freezes for as long as the request takes.
///
/// ⚠ CLAUDE: **`YAIOL_DEV` set ⇒ NOTHING IS SENT.** Not a marked request that gets filtered later:
/// no request at all. The developer's own machine is otherwise the largest distortion in the install
/// counts (the apps you build are the apps you open all day). A Rust app reads the variable directly
/// — there is no User-Agent trick as in electron, where the window cannot see the environment.
/// ⚠ A misspelt name fails silently (the request goes out and is counted as a real user).
pub fn check(app_id: &str, current: &str, skipped: &str) -> Option<String> {
    if std::env::var_os("YAIOL_DEV").is_some() {
        return None;
    }
    let url = format!("{ENDPOINT}/{app_id}/latest.json");
    let mut resp = ureq::get(&url)
        .header("Cache-Control", "no-cache")
        .config()
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .call()
        .ok()?;
    let body = resp.body_mut().read_to_string().ok()?;
    let latest = beacon_version(&body)?;
    engine::update_wanted(&latest, current, skipped).then_some(latest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_urls_use_the_web_language_form() {
        assert_eq!(site_url("my-app", "pt_BR", ""), "https://apps.yaiol.com/pt-BR/p/my-app/");
        assert_eq!(site_url("my-app", "en", "help"), "https://apps.yaiol.com/en/p/my-app/help/");
        assert_eq!(
            site_url("my-app", "fr", "/help//releases/"),
            "https://apps.yaiol.com/fr/p/my-app/help/releases/"
        );
    }

    #[test]
    fn the_beacon_is_version_only_and_tolerant() {
        assert_eq!(beacon_version(r#"{"version":"1.2.3","date":"2026-01-01"}"#).as_deref(), Some("1.2.3"));
        assert_eq!(beacon_version(r#"{"date":"x"}"#), None);
        assert_eq!(beacon_version(r#"{"version":""}"#), None);
        assert_eq!(beacon_version("<html>404</html>"), None);
    }
}
