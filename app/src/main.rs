// ⚠ CLAUDE - NO HARDCODED UI STRINGS IN THIS FILE.
//   Every string a user can read - text, title, aria-label - MUST go through the t() lookup, and
//   the key MUST be a string literal at the call site. Add the key to locales/en.json, then
//   translate, sort and audit every language with the i18n scripts. Never paste translations by hand.

mod catalog;
mod converter;
mod i18n;
mod icons;
mod manager;
mod notify;
mod runner;
mod settings;
mod update;

use catalog::{AppTab, GithubIcon, SettingsView, UpdateBanner};
use converter::Converter;
use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdAudioWaveform, LdBell, LdCircleHelp, LdEraser, LdListMusic, LdSettings, LdSlidersHorizontal},
};
use manager::{CleanupEditor, DestinationEditor, EffectsEditor, NotifyEditor, PresetEditor};

const CSS: [&str; 5] = [
    include_str!("../assets/ui-colors.css"),
    include_str!("../assets/ui-app.css"),
    include_str!("../assets/ui-listview.css"),
    include_str!("../assets/ui-ctl-knob.css"),
    include_str!("../assets/app.css"),
];

// ─── App identity - single source of truth: Cargo.toml (see build.rs) ───────────────────────────
const APP_NAME: &str = env!("YAIOL_PRODUCT_NAME");
const APP_ID: &str = env!("CARGO_PKG_NAME");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// This build's identity, `<version>.<counter>`, as ONE contiguous string that survives into the
/// executable. The apps-cockpit finds it by its `yaiol-build:` prefix to tell a buffered build from
/// the release it follows - the counter cannot ride in Cargo's three-part version, and `dx bundle`
/// writes its own Windows version resource, so this is the one channel that is entirely ours.
/// ⚠ CLAUDE: keep it a single `concat!` of literals, and read it through `black_box` (as
/// app_version_build does). A plain `BUILD_ID["yaiol-build:".len()..]` is constant-folded and the
/// optimiser keeps only the suffix - the PREFIX the cockpit searches for disappears from the exe and
/// every build reads as the bare release (found 2026-10-12 by grepping the staged exe).
const BUILD_ID: &str = concat!("yaiol-build:", env!("YAIOL_BUILD_ID"));

/// The full build identity shown to the user: the release version plus the build counter (the 4th
/// segment a buffered commit bumps), or just the release version on a clean build.
/// ⚠ CLAUDE: DISPLAY ONLY. The release version (`CARGO_PKG_VERSION`) stays plain three-part semver
/// and must not absorb the counter: an update check compares it against the published version, and
/// four parts is not semver.
fn app_version_build() -> String {
    let full = std::hint::black_box(BUILD_ID);
    let id = &full["yaiol-build:".len()..];
    id.strip_suffix(".0").unwrap_or(id).to_owned()
}

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            dioxus::desktop::Config::new()
                // ⚠ CLAUDE: the browser's own right-click menu (Back, Reload, Save as, Print, Inspect) is
                // never offered. Dioxus keeps it in a debug build by default - which is how it reached the
                // list on 2026-10-13 - and an app that wants a context menu draws its own.
                .with_disable_context_menu(true)
                .with_window(dioxus::desktop::WindowBuilder::new().with_title(APP_NAME))
                .with_menu(None::<dioxus::desktop::muda::Menu>),
        )
        .launch(App);
}

#[component]
fn App() -> Element {
    let mut settings = use_signal(settings::load);
    let mut settings_open = use_signal(|| false);
    // Which Settings tab is showing (and the one it opens on): display, presets, destinations, about.
    let mut settings_tab = use_signal(String::new);
    let mut update = use_signal(|| Option::<String>::None);

    // Once, on launch: is a newer release out that the user has not skipped? Blocking network, so it
    // runs off the UI thread; silent when offline, and nothing is sent at all under YAIOL_DEV.
    use_future(move || async move {
        let skipped = settings.peek().skip_update.clone();
        let found = tokio::task::spawn_blocking(move || update::check(APP_ID, APP_VERSION, &skipped))
            .await
            .ok()
            .flatten();
        if let Some(v) = found {
            update.set(Some(v));
        }
    });

    // <html> carries the language, its text direction and the theme: the catalog CSS keys off
    // `data-theme`, and a right-to-left language needs `dir` for the webview's own text engine.
    use_effect(move || {
        let s = settings.read();
        let dir = if engine::is_rtl(&s.lang) { "rtl" } else { "ltr" };
        let _ = document::eval(&format!(
            "const r=document.documentElement; r.setAttribute('lang','{}'); r.setAttribute('dir','{dir}'); r.setAttribute('data-theme','{}');",
            s.lang.replace('_', "-"),
            s.theme
        ));
    });

    // Every change persists immediately, so leaving a page is plain navigation.
    use_effect(move || settings::save(&settings.read()));

    let (lang, theme) = {
        let s = settings.read();
        (s.lang.clone(), s.theme.clone())
    };
    let t = |key: &str| i18n::t(&lang, key);
    let tip_help = t("tipHdrHelp");
    let tip_settings = t("tipHdrSettings");
    let tab_presets = t("tabStgPresets");
    let tab_destinations = t("tabStgDestinations");
    let tab_cleanups = t("tabStgCleanups");
    let tab_effects = t("tabStgEffects");
    let tab_notifications = t("tabStgNotifications");

    let version = app_version_build();
    let help_url = update::site_url(APP_ID, &lang, "help");
    let github_url = format!("https://github.com/yaiol/{APP_ID}");

    rsx! {
        for css in CSS {
            style { "{css}" }
        }
        div { class: "app-root",
            // Notify-only and dismissible. Skip remembers the version, so the banner stays quiet
            // until a newer one ships.
            if let Some(v) = update() {
                UpdateBanner {
                    version: v.clone(),
                    app_id: APP_ID,
                    lang: lang.clone(),
                    on_skip: move |_| {
                        settings.write().skip_update = v.clone();
                        update.set(None);
                    },
                    on_close: move |_| update.set(None),
                }
            }
            Converter {
                settings,
                app_name: APP_NAME,
                app_version: version.clone(),
                header_end: rsx! {
                    div { class: "barh-grp",
                        button {
                            class: "btn icon",
                            title: "GitHub",
                            "aria-label": "GitHub",
                            onclick: move |_| {
                                let _ = open::that(&github_url);
                            },
                            GithubIcon {}
                        }
                        button {
                            class: "btn icon",
                            title: "{tip_help}",
                            "aria-label": "{tip_help}",
                            onclick: move |_| {
                                let _ = open::that(&help_url);
                            },
                            Icon { width: 14, height: 14, icon: LdCircleHelp }
                        }
                        button {
                            class: if settings_open() { "btn icon stg-toggle active" } else { "btn icon stg-toggle" },
                            title: "{tip_settings}",
                            "aria-label": "{tip_settings}",
                            "aria-pressed": "{settings_open()}",
                            onclick: move |_| {
                                settings_tab.set(String::new());
                                settings_open.toggle();
                            },
                            Icon { width: 14, height: 14, icon: LdSettings }
                        }
                    }
                },
                // The bottom bar's edit buttons open Settings on the right tab.
                on_manage: move |tab: String| {
                    settings_tab.set(tab);
                    settings_open.set(true);
                },
                if settings_open() {
                    SettingsView {
                        app_name: APP_NAME,
                        app_version: version.clone(),
                        lang: lang.clone(),
                        theme: theme.clone(),
                        on_set_lang: move |l| settings.write().lang = l,
                        on_set_theme: move |th| settings.write().theme = th,
                        on_close: move |_| settings_open.set(false),
                        app_tabs: vec![
                            AppTab {
                                key: manager::PRESETS.to_owned(),
                                label: tab_presets.clone(),
                                icon: rsx! { Icon { width: 14, height: 14, icon: LdSlidersHorizontal } },
                            },
                            AppTab {
                                key: manager::CLEANUPS.to_owned(),
                                label: tab_cleanups.clone(),
                                icon: rsx! { Icon { width: 14, height: 14, icon: LdEraser } },
                            },
                            AppTab {
                                key: manager::EFFECTS.to_owned(),
                                label: tab_effects.clone(),
                                icon: rsx! { Icon { width: 14, height: 14, icon: LdAudioWaveform } },
                            },
                            AppTab {
                                key: manager::DESTINATIONS.to_owned(),
                                label: tab_destinations.clone(),
                                icon: rsx! { Icon { width: 14, height: 14, icon: LdListMusic } },
                            },
                            AppTab {
                                key: manager::NOTIFICATIONS.to_owned(),
                                label: tab_notifications.clone(),
                                icon: rsx! { Icon { width: 14, height: 14, icon: LdBell } },
                            },
                        ],
                        initial_tab: settings_tab(),
                        on_tab: move |key: String| settings_tab.set(key),
                        app_page: Some(rsx! {
                            if settings_tab() == manager::PRESETS {
                                PresetEditor { settings }
                            } else if settings_tab() == manager::DESTINATIONS {
                                DestinationEditor { settings }
                            } else if settings_tab() == manager::CLEANUPS {
                                CleanupEditor { settings }
                            } else if settings_tab() == manager::EFFECTS {
                                EffectsEditor { settings }
                            } else if settings_tab() == manager::NOTIFICATIONS {
                                NotifyEditor { settings }
                            }
                        }),
                    }
                }
            }
        }
    }
}
