//! Everything the window shows about ffmpeg: the section of Settings > General (which version is installed, and
//! the one button that brings it up to date), and the popup that appears while an install or an update runs, or
//! when one fails. There is nothing to choose: the app runs the one build yaiol approved, from the folder every
//! yaiol app shares, and installs it by itself when it is missing or when a newer one is approved.
//!
//! ⚠ CLAUDE: NO HARDCODED UI STRINGS. Everything goes through the t() lookup with a literal key at the call site.

use crate::{
    i18n,
    runner::{DOWNLOAD, LATEST},
};
use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdCheck, LdDownload, LdRefreshCw},
};
use engine::download::{self, DownloadState, LatestState};
use std::time::Duration;

/// The version the install recorded in the shared folder, when there is one.
pub fn installed_version() -> Option<String> {
    engine::settings::ffmpeg_dir().and_then(|dir| download::installed_version(&dir))
}

/// The installed pair is exactly the one the manifest names (the same ffmpeg and the same catalogue).
pub fn is_current(manifest: &download::Manifest) -> bool {
    engine::settings::ffmpeg_dir().is_some_and(|dir| download::is_current(&dir, manifest))
}

/// What the window says about an install that is running or has failed; `None` when nothing is happening.
pub fn download_note(lang: &str, state: &DownloadState) -> Option<String> {
    let t = |key: &str| i18n::t(lang, key);
    match state {
        DownloadState::Downloading { done, total } => {
            let mb = |n: u64| (n / (1024 * 1024)).to_string();
            Some(t("msgMainDownloading").replace("{done}", &mb(*done)).replace("{total}", &total.map_or_else(|| "?".to_owned(), mb)))
        }
        DownloadState::Verifying | DownloadState::Unpacking => Some(t("msgMainDownloadFinishing")),
        DownloadState::Failed(reason) => Some(t("msgMainDownloadFailed").replace("{reason}", reason)),
        DownloadState::Idle | DownloadState::Done(_) => None,
    }
}

/// Start installing the manifest's ffmpeg into the shared folder.
pub fn install() {
    if let Some(dir) = engine::settings::ffmpeg_dir() {
        DOWNLOAD.start(dir);
    }
}

/// What the popup says; each carries its text, and whether an ffmpeg is already in place (an update, not a first install).
#[derive(Clone, PartialEq)]
pub enum Popup {
    /// An install is running: no button, it closes by itself when it ends.
    Progress { text: String, update: bool },
    /// An install failed: try again, or close.
    Failed { text: String, update: bool },
}

#[component]
pub fn FfmpegPopup(lang: String, popup: Popup, on_close: EventHandler<()>) -> Element {
    let t = |key: &str| i18n::t(&lang, key);
    let (retry, close) = (t("btnMainFfmpegRetry"), t("btnMainFfmpegClose"));
    let (update, text, action) = match &popup {
        Popup::Progress { text, update } => (*update, text.clone(), None),
        Popup::Failed { text, update } => (*update, text.clone(), Some(retry)),
    };
    let title = if update { t("ttlMainFfmpegUpdating") } else { t("ttlMainFfmpegInstalling") };
    rsx! {
        div { class: "dl-backdrop",
            div { class: "dlp",
                Icon { icon: LdDownload, class: "dlp-icon" }
                div { class: "dlp-title", "{title}" }
                div { class: "dlp-sub", "{text}" }
                if let Some(action) = action {
                    div { class: "dlp-foot",
                        button { class: "btn", onclick: move |_| install(), "{action}" }
                        button { class: "btn", onclick: move |_| on_close.call(()), "{close}" }
                    }
                }
            }
        }
    }
}

#[component]
pub fn FfmpegSetting(lang: String) -> Element {
    let mut installed = use_signal(installed_version);
    let mut latest = use_signal(|| LATEST.snapshot());
    let mut dl = use_signal(|| DOWNLOAD.snapshot());
    // Asked again each time Settings opens (this section mounts with the page).
    use_hook(|| LATEST.check());
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let l = LATEST.snapshot();
            if *latest.peek() != l {
                latest.set(l);
            }
            let d = DOWNLOAD.snapshot();
            if *dl.peek() != d {
                if matches!(d, DownloadState::Done(_)) {
                    installed.set(installed_version());
                }
                dl.set(d);
            }
        }
    });

    let t = |key: &str| i18n::t(&lang, key);
    let have = installed();
    let busy = dl().is_active();
    let latest = latest();
    let up_to_date = matches!(&latest, LatestState::Known(m) if have.is_some() && is_current(m));
    let latest_line = match &latest {
        LatestState::Unknown | LatestState::Checking => t("msgStgFfmpegChecking"),
        LatestState::Failed(_) => t("msgStgFfmpegCheckFailed"),
        LatestState::Known(m) => t("msgStgFfmpegLatest").replace("{version}", &m.version),
    };
    // What the button does, said once the answer is in: the version, the size and where it comes from.
    let explain = match &latest {
        LatestState::Known(m) => m.build().map(|b| {
            t("msgStgFfmpegExplain")
                .replace("{version}", &m.version)
                .replace("{size}", &(b.size / (1024 * 1024)).to_string())
                .replace("{source}", &b.source)
        }),
        _ => None,
    };
    let installed_line = match &have {
        Some(v) => t("msgStgFfmpegInstalled").replace("{version}", v),
        None => t("msgStgFfmpegNone"),
    };
    let label = t("lblStgFfmpeg");
    let button = if have.is_some() { t("btnMainUpdateFfmpeg") } else { t("btnMainDownloadFfmpeg") };
    let note = download_note(&lang, &dl());
    let offer = matches!(latest, LatestState::Known(_)) && !up_to_date;

    rsx! {
        div { class: "dlg-field divider",
            label { class: "dlg-field-label", "{label}" }
            span {
                "{installed_line}"
                if up_to_date {
                    " "
                    Icon { width: 14, height: 14, icon: LdCheck }
                }
            }
            if !up_to_date {
                span { "{latest_line}" }
            }
            if offer {
                div {
                    button {
                        class: "btn",
                        disabled: busy,
                        onclick: move |_| install(),
                        if have.is_some() {
                            Icon { width: 14, height: 14, icon: LdRefreshCw }
                        } else {
                            Icon { width: 14, height: 14, icon: LdDownload }
                        }
                        " {button}"
                    }
                }
                if let Some(text) = explain {
                    span { "{text}" }
                }
            }
            if let Some(text) = note {
                span { "{text}" }
            }
        }
    }
}
