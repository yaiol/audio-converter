//! The main view: the file list, the drop target, Convert, and the bar with the preset, the output
//! destination and the number of conversions at once.
//!
//! ⚠ CLAUDE: NO HARDCODED UI STRINGS. Every user-visible string goes through the t() lookup with the key
//! a string literal at the call site.

use crate::{
    catalog::{AppHeader, DropZone},
    i18n, manager,
    runner::RUNNER,
    settings::Settings,
};
use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdFilePlus, LdFolderOpen, LdFolderPlus, LdMusic, LdPlay, LdSlidersHorizontal, LdSquare, LdX},
};
use engine::{JobSpec, JobStatus, Overwrite, destination::SOURCE_ID};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, PartialEq)]
struct Item {
    id: u64,
    name: String,
    path: PathBuf,
    /// The folder this file was added through; None for a file added on its own.
    root: Option<PathBuf>,
}

/// Every audio file under `path` (or `path` itself), in a stable order.
fn collect_audio(path: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if path.is_dir() {
        if depth > 12 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(path) else { return };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for e in entries {
            collect_audio(&e, out, depth + 1);
        }
    } else if path
        .file_name()
        .is_some_and(|n| engine::is_audio_file(&n.to_string_lossy()))
    {
        out.push(path.to_path_buf());
    }
}

fn percent_text(p: f32) -> String {
    format!("{} %", p.round() as u32)
}

/// How a run ended: how many were converted, skipped, failed - `3 converted · 1 skipped · 2 failed`.
/// Empty when none of the three happened (nothing ran, or it was only cancelled).
fn run_summary<'a>(statuses: impl Iterator<Item = &'a JobStatus>, lang: &str) -> String {
    let (mut done, mut skipped, mut failed) = (0, 0, 0);
    for s in statuses {
        match s {
            JobStatus::Done { .. } => done += 1,
            JobStatus::Skipped { .. } => skipped += 1,
            JobStatus::Failed { .. } => failed += 1,
            _ => {}
        }
    }
    let t = |key: &str| i18n::t(lang, key);
    let mut parts = Vec::new();
    if done > 0 {
        parts.push(format!("{done} {}", t("lblMainNotifyDone")));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} {}", t("lblMainNotifySkipped")));
    }
    if failed > 0 {
        parts.push(format!("{failed} {}", t("lblMainNotifyFailed")));
    }
    parts.join(" · ")
}

/// One operating-system notice for a finished run. A run that was only cancelled says nothing.
fn notify_finished(snap: &HashMap<u64, JobStatus>, lang: &str) {
    let body = run_summary(snap.values(), lang);
    if body.is_empty() {
        return;
    }
    let title = i18n::t(lang, "ttlMainNotify");
    std::thread::spawn(move || crate::notify::show(&title, &body));
}

#[component]
pub fn Converter(
    settings: Signal<Settings>,
    app_name: String,
    app_version: String,
    /// The header's right-hand group (GitHub, help, settings) - the app owns those.
    header_end: Element,
    on_manage: EventHandler<String>,
    children: Element,
) -> Element {
    let mut items = use_signal(Vec::<Item>::new);
    let mut statuses = use_signal(HashMap::<u64, JobStatus>::new);
    // Cover thumbnails as data URIs. A present key with None means "looked, there is none".
    let mut thumbs = use_signal(HashMap::<u64, Option<String>>::new);

    // A few files at a time, off the UI thread, so a folder of hundreds fills in without a stall. The
    // picture is read in-process (no process per file); ffmpeg is only the fallback for what the
    // tag library cannot read.
    use_future(move || async move {
        const AT_ONCE: usize = 4;
        loop {
            let batch: Vec<(u64, PathBuf)> = {
                let done = thumbs.read();
                items
                    .read()
                    .iter()
                    .filter(|i| !done.contains_key(&i.id))
                    .take(AT_ONCE)
                    .map(|i| (i.id, i.path.clone()))
                    .collect()
            };
            if batch.is_empty() {
                tokio::time::sleep(Duration::from_millis(250)).await;
                continue;
            }
            let jobs: Vec<_> = batch
                .into_iter()
                .map(|(id, path)| {
                    let ff = crate::runner::FFMPEG.clone();
                    let handle = tokio::task::spawn_blocking(move || {
                        use base64::{Engine, engine::general_purpose::STANDARD};
                        engine::thumbnail::cover_thumbnail(ff.as_deref(), &path, 96)
                            .map(|b| format!("data:image/jpeg;base64,{}", STANDARD.encode(b)))
                    });
                    (id, handle)
                })
                .collect();
            // One write for the whole batch: each write of this signal re-renders the list.
            let mut finished = Vec::with_capacity(jobs.len());
            for (id, handle) in jobs {
                finished.push((id, handle.await.ok().flatten()));
            }
            thumbs.write().extend(finished);
        }
    });

    // The window polls the runner; the engine never calls back into it. Only a CHANGED snapshot is
    // written, so an idle queue does not re-render the list five times a second.
    use_future(move || async move {
        let mut was_active = false;
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if let Some(r) = RUNNER.as_ref() {
                let snap: HashMap<u64, JobStatus> = r.snapshot().into_iter().collect();
                // A run just ended: tell the user, whichever window they are looking at.
                let active = snap.values().any(JobStatus::is_active);
                if was_active && !active && settings.peek().notify {
                    notify_finished(&snap, &settings.peek().lang);
                }
                was_active = active;
                if *statuses.peek() != snap {
                    statuses.set(snap);
                }
            }
        }
    });

    let lang = settings.read().lang.clone();
    let t = |key: &str| i18n::t(&lang, key);

    let mut add_paths = move |paths: Vec<PathBuf>| {
        let mut list = items.write();
        for p in &paths {
            // A dropped / picked FOLDER is the root its files keep their sub-folders relative to.
            let root = p.is_dir().then(|| p.clone());
            let mut found = Vec::new();
            collect_audio(p, &mut found, 0);
            for path in found {
                if list.iter().any(|i| i.path == path) {
                    continue;
                }
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                list.push(Item { id: NEXT_ID.fetch_add(1, Ordering::Relaxed), name, path, root: root.clone() });
            }
        }
    };

    let pick_files = move |_| {
        spawn(async move {
            let exts: Vec<&str> = engine::AUDIO_EXTENSIONS.to_vec();
            if let Some(files) = rfd::AsyncFileDialog::new().add_filter("audio", &exts).pick_files().await {
                add_paths(files.into_iter().map(|f| f.path().to_path_buf()).collect());
            }
        });
    };
    let pick_folder = move |_| {
        spawn(async move {
            if let Some(dir) = rfd::AsyncFileDialog::new().pick_folder().await {
                add_paths(vec![dir.path().to_path_buf()]);
            }
        });
    };

    let ffmpeg_missing = RUNNER.is_none();
    let busy = statuses.read().values().any(JobStatus::is_active);

    let convert = move |_| {
        let Some(runner) = RUNNER.as_ref() else { return };
        let s = settings.read();
        let preset = s.selected_preset();
        let destination = s.selected_destination();
        let cleanup = s.selected_cleanup();
        let effects = s.selected_effects();
        let overwrite = s.overwrite;
        let parallel = s.parallel as usize;
        drop(s);
        let st = statuses.peek();
        let specs: Vec<JobSpec> = items
            .read()
            .iter()
            // Active ones are already queued or running; everything else - new, done, failed,
            // skipped, cancelled - runs again with the current settings.
            .filter(|i| !st.get(&i.id).is_some_and(JobStatus::is_active))
            .map(|i| JobSpec {
                id: i.id,
                input: i.path.clone(),
                source_root: i.root.clone(),
                preset: preset.clone(),
                cleanup: cleanup.clone(),
                destination: destination.clone(),
                overwrite,
                effects: effects.clone(),
            })
            .collect();
        drop(st);
        runner.start(specs, parallel);
    };
    let stop = move |_| {
        if let Some(r) = RUNNER.as_ref() {
            r.cancel_all();
        }
    };
    // Empties the list except what is queued or converting right now: a file that was never started
    // has no status at all, and it is the one a user most often wants gone.
    let clear_list = move |_| {
        let st = statuses.peek().clone();
        let idle = |id: &u64| st.get(id).map_or(true, |s| !s.is_active());
        if let Some(r) = RUNNER.as_ref() {
            for i in items.read().iter().filter(|i| idle(&i.id)) {
                r.remove(i.id);
            }
        }
        items.write().retain(|i| !idle(&i.id));
    };

    let have_items = !items.read().is_empty();
    let any_idle = items.read().iter().any(|i| statuses.read().get(&i.id).map_or(true, |s| !s.is_active()));

    let s = settings.read();
    // In the saved order - the user arranges it by dragging, in Settings.
    // "(Original)", "(None)" and "(None)" come first and are not entries of Settings.
    let mut preset_options: Vec<(String, String)> = vec![(engine::preset::ORIGINAL_ID.to_owned(), t("lblMainPresetOriginal"))];
    preset_options.extend(s.presets.iter().map(|p| (p.id.clone(), p.name.clone())));
    let dest_options: Vec<(String, String)> = s.destinations.iter().map(|d| (d.id.clone(), d.name.clone())).collect();
    let mut cleanup_options: Vec<(String, String)> = vec![(crate::settings::NONE_ID.to_owned(), t("lblMainNone"))];
    cleanup_options.extend(s.cleanups.iter().map(|c| (c.id.clone(), c.name.clone())));
    let cleanup_id = s.cleanup_id.clone();
    let mut effects_options: Vec<(String, String)> = vec![(crate::settings::NONE_ID.to_owned(), t("lblMainNone"))];
    effects_options.extend(s.effects.iter().map(|e| (e.id.clone(), e.name.clone())));
    // Keep the format, keep the tags, apply nothing: converting would only re-encode, so there is
    // nothing to do and the button is off.
    let nothing_to_do = {
        let (c, e) = (s.selected_cleanup(), s.selected_effects());
        s.selected_preset().is_original() && !c.edits_tags() && !c.remove_cover && e.items.is_empty()
    };
    let effects_id = s.effects_id.clone();
    let preset_id = s.preset_id.clone();
    let destination_id = s.destination_id.clone();
    // Where "open the output folder" goes: the folder of the newest finished file - which is right
    // for the built-in "same folder" too - and otherwise the saved destination's own folder.
    let saved_folder = s
        .destinations
        .iter()
        .find(|d| d.id == destination_id)
        .map(|d| PathBuf::from(d.folder.trim()))
        .filter(|p| !p.as_os_str().is_empty());
    let parallel = s.parallel;
    let overwrite = s.overwrite;
    drop(s);

    let s_main_add_files = t("btnMainAddFiles");
    let s_main_add_folder = t("btnMainAddFolder");
    let s_main_clear = t("btnMainClear");
    let s_main_stop = t("btnMainStop");
    let s_main_convert = t("btnMainConvert");
    let s_main_preset = t("lblMainPreset");
    let s_main_edit_presets = t("tipMainEditPresets");
    let s_main_destination = t("tipMainDestination");
    // The left of the footer: what the list holds, how far a run is, or how the last run ended.
    let status_text = {
        let st = statuses.read();
        let total = items.read().len();
        let touched = items.read().iter().filter(|i| st.contains_key(&i.id)).count();
        if total == 0 {
            String::new()
        } else if busy {
            let finished = st.values().filter(|s| !s.is_active()).count();
            t("msgMainProgress").replace("{done}", &finished.to_string()).replace("{total}", &touched.to_string())
        } else if touched > 0 {
            run_summary(st.values(), &settings.read().lang)
        } else {
            t("msgMainReady").replace("{n}", &total.to_string())
        }
    };
    let s_main_edit_destinations = t("tipMainEditDestinations");
    let s_main_cleanup = t("tipMainCleanup");
    let s_main_edit_cleanups = t("tipMainEditCleanups");
    let s_main_effects = t("tipMainEffects");
    let s_main_edit_effects = t("tipMainEditEffects");
    let s_main_open_output = t("tipMainOpenOutput");
    let open_target: Option<PathBuf> = statuses
        .read()
        .iter()
        .filter_map(|(id, s)| match s {
            JobStatus::Done { output } => Some((*id, output.clone())),
            _ => None,
        })
        .max_by_key(|(id, _)| *id)
        .and_then(|(_, out)| out.parent().map(Path::to_path_buf))
        .or(saved_folder)
        .filter(|p| p.is_dir());
    let s_main_overwrite = t("tipMainOverwrite");
    let s_main_overwrite_skip = t("lblMainOverwriteSkip");
    let s_main_overwrite_overwrite = t("lblMainOverwriteOverwrite");
    let s_main_overwrite_rename = t("lblMainOverwriteRename");
    let s_main_parallel = t("tipMainParallel");
    let list_empty = t("msgMainListEmpty");
    let no_ffmpeg = t("msgMainNoFfmpeg");
    let source_label = t("lblMainDestSource");

    let drop_label = t("msgMainDropLabel");

    rsx! {
        // ⚠ CLAUDE: the file actions live in the ONE app header, icon-only with a tooltip. A second bar
        // under it was removed on request; do not bring it back.
        AppHeader { app_name, app_version,
            div { class: "barh-grp",
                button {
                    class: "btn icon",
                    title: "{s_main_add_files}",
                    "aria-label": "{s_main_add_files}",
                    onclick: pick_files,
                    Icon { width: 14, height: 14, icon: LdFilePlus }
                }
                button {
                    class: "btn icon",
                    title: "{s_main_add_folder}",
                    "aria-label": "{s_main_add_folder}",
                    onclick: pick_folder,
                    Icon { width: 14, height: 14, icon: LdFolderPlus }
                }
                button {
                    class: "btn icon",
                    title: "{s_main_clear}",
                    "aria-label": "{s_main_clear}",
                    disabled: !any_idle,
                    onclick: clear_list,
                    Icon { width: 14, height: 14, icon: crate::icons::LdBrushCleaning }
                }
            }
            div { class: "barh-grp",
                select {
                    class: "select",
                    title: "{s_main_preset}",
                    "aria-label": "{s_main_preset}",
                    value: "{preset_id}",
                    onchange: move |e| settings.write().preset_id = e.value(),
                    for (id, name) in preset_options {
                        option { key: "{id}", value: "{id}", selected: id == preset_id, "{name}" }
                    }
                }
                button {
                    class: "btn icon",
                    title: "{s_main_edit_presets}",
                    "aria-label": "{s_main_edit_presets}",
                    onclick: move |_| on_manage.call(manager::PRESETS.to_owned()),
                    Icon { width: 14, height: 14, icon: LdSlidersHorizontal }
                }
            }
            div { class: "barh-grp",
                select {
                    class: "select",
                    title: "{s_main_cleanup}",
                    "aria-label": "{s_main_cleanup}",
                    value: "{cleanup_id}",
                    onchange: move |e| settings.write().cleanup_id = e.value(),
                    for (id, name) in cleanup_options {
                        option { key: "{id}", value: "{id}", selected: id == cleanup_id, "{name}" }
                    }
                }
                button {
                    class: "btn icon",
                    title: "{s_main_edit_cleanups}",
                    "aria-label": "{s_main_edit_cleanups}",
                    onclick: move |_| on_manage.call(manager::CLEANUPS.to_owned()),
                    Icon { width: 14, height: 14, icon: LdSlidersHorizontal }
                }
            }
            div { class: "barh-grp",
                select {
                    class: "select",
                    title: "{s_main_effects}",
                    "aria-label": "{s_main_effects}",
                    value: "{effects_id}",
                    onchange: move |e| settings.write().effects_id = e.value(),
                    for (id, name) in effects_options {
                        option { key: "{id}", value: "{id}", selected: id == effects_id, "{name}" }
                    }
                }
                button {
                    class: "btn icon",
                    title: "{s_main_edit_effects}",
                    "aria-label": "{s_main_edit_effects}",
                    onclick: move |_| on_manage.call(manager::EFFECTS.to_owned()),
                    Icon { width: 14, height: 14, icon: LdSlidersHorizontal }
                }
            }
            div { class: "barh-grp",
                select {
                    class: "select",
                    title: "{s_main_parallel}",
                    "aria-label": "{s_main_parallel}",
                    value: "{parallel}",
                    onchange: move |e| {
                        if let Ok(n) = e.value().parse::<u32>() {
                            settings.write().parallel = n.clamp(1, crate::settings::max_parallel());
                        }
                    },
                    for n in 1..=crate::settings::max_parallel() {
                        option { key: "{n}", value: "{n}", selected: n == parallel, "{n}" }
                    }
                }
            }
            div { class: "barh-grp",
                if busy {
                    button {
                        class: "btn icon",
                        title: "{s_main_stop}",
                        "aria-label": "{s_main_stop}",
                        onclick: stop,
                        Icon { width: 14, height: 14, icon: LdSquare }
                    }
                }
                button {
                    class: "btn icon",
                    title: "{s_main_convert}",
                    "aria-label": "{s_main_convert}",
                    disabled: ffmpeg_missing || !have_items || busy || nothing_to_do,
                    onclick: convert,
                    Icon { width: 14, height: 14, icon: LdPlay }
                }
            }
            span { class: "bar-spacer" }
            {header_end}
        }
        DropZone {
            class: "app-main",
            label: drop_label,
            on_files: move |dropped: Vec<(String, String)>| {
                add_paths(dropped.into_iter().map(|(_, p)| PathBuf::from(p)).collect());
            },
            {children}
        if ffmpeg_missing {
            div { class: "workspace-empty converter-note", "{no_ffmpeg}" }
        }
        div { class: "lv-scroll converter-list",
            if !have_items {
                p { class: "workspace-empty", "{list_empty}" }
            }
            for item in items.read().iter().cloned() {
                {
                    let status = statuses.read().get(&item.id).cloned();
                    let active = status.as_ref().is_some_and(JobStatus::is_active);
                    let (badge, detail, state) = match &status {
                        None => (String::new(), String::new(), "idle"),
                        Some(JobStatus::Queued) => (t("msgMainStatusQueued"), String::new(), "idle"),
                        Some(JobStatus::Probing) => (t("msgMainStatusReading"), String::new(), "idle"),
                        Some(JobStatus::Analysing { percent }) => (format!("{} {}", t("msgMainStatusAnalysing"), percent_text(*percent)), String::new(), "run"),
                        Some(JobStatus::Running { percent }) => (percent_text(*percent), String::new(), "run"),
                        Some(JobStatus::Done { output }) => (t("msgMainStatusDone"), output.display().to_string(), "ok"),
                        Some(JobStatus::Skipped { output }) => (t("msgMainStatusSkipped"), output.display().to_string(), "idle"),
                        Some(JobStatus::Cancelled) => (t("msgMainStatusCancelled"), String::new(), "idle"),
                        Some(JobStatus::Failed { reason }) => (t("msgMainStatusFailed"), reason.clone(), "err"),
                    };
                    let id = item.id;
                    let path = item.path.display().to_string();
                    let tip = if active { t("tipMainCancel") } else { t("tipMainRemove") };
                    rsx! {
                        div { key: "{id}", class: "lv-item converter-row",
                            div { class: "lv-item-icon converter-cover-box",
                                if let Some(Some(src)) = thumbs.read().get(&id) {
                                    img { class: "converter-cover", src: "{src}", alt: "" }
                                } else {
                                    Icon { width: 18, height: 18, icon: LdMusic }
                                }
                            }
                            div { class: "lv-item-body",
                                div { class: "lv-item-name", "{item.name}" }
                                div { class: "lv-item-sub", "{path}" }
                            }
                            if !badge.is_empty() {
                                span { class: "lv-item-badge converter-state converter-state-{state}", title: "{detail}", "{badge}" }
                            }
                            button {
                                class: "btn icon subtle",
                                title: "{tip}",
                                "aria-label": "{tip}",
                                onclick: move |_| {
                                    if active {
                                        if let Some(r) = RUNNER.as_ref() {
                                            r.cancel(id);
                                        }
                                    } else {
                                        if let Some(r) = RUNNER.as_ref() {
                                            r.remove(id);
                                        }
                                        items.write().retain(|i| i.id != id);
                                    }
                                },
                                Icon { width: 14, height: 14, icon: LdX }
                            }
                        }
                    }
                }
            }
        }
        }
        // Everything sits at the right end, tooltips instead of labels: the output choice and its settings
        // are one welded group like the header's, and the open-folder icon is the last thing on the bar.
        div { class: "barh-footer",
            span { class: "converter-status", "{status_text}" }
            span { class: "bar-spacer" }
            div { class: "barh-grp",
                select {
                    class: "select",
                    title: "{s_main_destination}",
                    "aria-label": "{s_main_destination}",
                    value: "{destination_id}",
                    onchange: move |e| settings.write().destination_id = e.value(),
                    option { value: "{SOURCE_ID}", selected: destination_id == SOURCE_ID, "{source_label}" }
                    for (id, name) in dest_options {
                        option { key: "{id}", value: "{id}", selected: id == destination_id, "{name}" }
                    }
                }
                button {
                    class: "btn icon",
                    title: "{s_main_edit_destinations}",
                    "aria-label": "{s_main_edit_destinations}",
                    onclick: move |_| on_manage.call(manager::DESTINATIONS.to_owned()),
                    Icon { width: 14, height: 14, icon: LdSlidersHorizontal }
                }
            }
            select {
                class: "select",
                title: "{s_main_overwrite}",
                "aria-label": "{s_main_overwrite}",
                value: match overwrite { Overwrite::Skip => "skip", Overwrite::Overwrite => "overwrite", Overwrite::Rename => "rename" },
                onchange: move |e| {
                    settings.write().overwrite = match e.value().as_str() {
                        "overwrite" => Overwrite::Overwrite,
                        "rename" => Overwrite::Rename,
                        _ => Overwrite::Skip,
                    };
                },
                option { value: "skip", selected: overwrite == Overwrite::Skip, "{s_main_overwrite_skip}" }
                option { value: "overwrite", selected: overwrite == Overwrite::Overwrite, "{s_main_overwrite_overwrite}" }
                option { value: "rename", selected: overwrite == Overwrite::Rename, "{s_main_overwrite_rename}" }
            }
            button {
                class: "btn icon",
                title: "{s_main_open_output}",
                "aria-label": "{s_main_open_output}",
                disabled: open_target.is_none(),
                onclick: move |_| {
                    if let Some(p) = &open_target {
                        let _ = open::that(p);
                    }
                },
                Icon { width: 14, height: 14, icon: LdFolderOpen }
            }
        }
    }
}

