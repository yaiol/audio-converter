//! The main view: the file list, the drop target, Convert, and the bar with the preset, the output
//! destination and the number of conversions at once.
//!
//! ⚠ CLAUDE: NO HARDCODED UI STRINGS. Every user-visible string goes through the t() lookup with the key
//! a string literal at the call site.

use crate::{
    catalog::{AppHeader, DropZone},
    i18n, manager,
    runner::RUNNER,
    settings::{RunChoice, Settings},
};
use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdAudioWaveform, LdEraser, LdFilePlus, LdFolderOpen, LdFolderPlus, LdListChecks, LdListMusic, LdMusic, LdPlay, LdReplace, LdSlidersHorizontal, LdSquare, LdX},
};
use engine::{JobSpec, JobStatus, Overwrite, destination::SOURCE_ID};
use std::{
    collections::{HashMap, HashSet},
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
    /// What this file will be converted with - the panel's choices when it was added or last applied to it.
    run: RunChoice,
}

/// The rows from `a` to `b` in list order, both included (the order the user sees, whichever end came first).
fn range_between(ids: &[u64], a: u64, b: u64) -> Vec<u64> {
    let (Some(x), Some(y)) = (ids.iter().position(|i| *i == a), ids.iter().position(|i| *i == b)) else {
        return vec![b];
    };
    ids[x.min(y)..=x.max(y)].to_vec()
}

/// One tag of a row: the control's icon and the name of what was chosen.
#[derive(Clone, PartialEq)]
enum TagKind {
    Preset,
    Cleanup,
    Effects,
    Destination,
    Overwrite,
}

#[component]
fn RunTag(kind: TagKind, text: String, title: String) -> Element {
    rsx! {
        span { class: "chip", title: "{title}",
            match kind {
                TagKind::Preset => rsx! { Icon { width: 14, height: 14, icon: LdSlidersHorizontal } },
                TagKind::Cleanup => rsx! { Icon { width: 14, height: 14, icon: LdEraser } },
                TagKind::Effects => rsx! { Icon { width: 14, height: 14, icon: LdAudioWaveform } },
                TagKind::Destination => rsx! { Icon { width: 14, height: 14, icon: LdListMusic } },
                TagKind::Overwrite => rsx! { Icon { width: 14, height: 14, icon: LdReplace } },
            }
            "{text}"
        }
    }
}

/// Keeping the format, every tag and the cover, and applying no effect would only re-encode the file.
fn nothing_to_do(s: &Settings, run: &RunChoice) -> bool {
    let (c, e) = (s.cleanup_by(&run.cleanup_id), s.effects_by(&run.effects_id));
    s.preset_by(&run.preset_id).is_original() && !c.edits_tags() && !c.remove_cover && e.items.is_empty()
}

fn percent_text(p: f32) -> String {
    format!("{} %", p.round() as u32)
}

/// One operating-system notice for a finished run. A run that was only cancelled says nothing.
fn notify_finished(snap: &HashMap<u64, JobStatus>, lang: &str) {
    let body = engine::run_summary(snap.values(), lang);
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
    // The rows the panel's Apply writes to, and the one a Shift+click counts from.
    let mut selected = use_signal(HashSet::<u64>::new);
    let mut anchor = use_signal(|| None::<u64>);

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

    // The window may not be shorter than the layout needs: the header, the options panel down to its
    // last row, and the status bar. Measured from the laid-out page - the panel's rows decide it, so a
    // row added there raises the floor by itself. The first frames may not be laid out yet (fonts,
    // styles), so it is asked again until the panel has a real height; if it never answers, a fixed
    // floor stands, so the window is never left without one.
    use_future(move || async move {
        let mut height = 0.0;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let mut ev = document::eval(
                r#"
                const q = (s) => document.querySelector(s);
                const panel = q('.converter-panel'), head = q('.barh-header');
                const rule = q('.barh-gradient'), foot = q('.barh-footer');
                if (!panel || !head || !foot || !panel.lastElementChild) { dioxus.send(0); }
                else {
                    const end = panel.lastElementChild.getBoundingClientRect().bottom;
                    const pad = parseFloat(getComputedStyle(panel).paddingBottom) || 0;
                    dioxus.send(head.offsetHeight + (rule ? rule.offsetHeight : 0)
                        + (end - panel.getBoundingClientRect().top) + pad + foot.offsetHeight);
                }
                "#,
            );
            if let Ok(h) = ev.recv::<f64>().await {
                if h > 100.0 {
                    height = h.ceil();
                    break;
                }
            }
        }
        if height == 0.0 {
            height = crate::MIN_HEIGHT_FALLBACK;
        }
        dioxus::desktop::window().set_min_inner_size(Some(dioxus::desktop::LogicalSize::new(crate::MIN_WIDTH, height)));
    });

    let lang = settings.read().lang.clone();
    let t = |key: &str| i18n::t(&lang, key);

    let mut add_paths = move |paths: Vec<PathBuf>| {
        // Added files take the panel's choices as they are now, and become the selection.
        let run = settings.read().choice();
        let mut fresh = Vec::new();
        let mut list = items.write();
        for p in &paths {
            // A dropped / picked FOLDER is the root its files keep their sub-folders relative to.
            let root = p.is_dir().then(|| p.clone());
            let mut found = Vec::new();
            engine::collect_audio(p, &mut found);
            for path in found {
                if list.iter().any(|i| i.path == path) {
                    continue;
                }
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                fresh.push(id);
                list.push(Item { id, name, path, root: root.clone(), run: run.clone() });
            }
        }
        if let Some(last) = fresh.last().copied() {
            selected.set(fresh.into_iter().collect());
            anchor.set(Some(last));
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

    // Every file runs with ITS OWN choices; only the number at once belongs to the run. A file whose
    // choices would only re-encode it as it is has nothing to do and is left out.
    let convert = move |_| {
        let Some(runner) = RUNNER.as_ref() else { return };
        let s = settings.read();
        let parallel = s.parallel as usize;
        let st = statuses.peek();
        let specs: Vec<JobSpec> = items
            .read()
            .iter()
            // Active ones are already queued or running; everything else - new, done, failed,
            // skipped, cancelled - runs again with its current choices.
            .filter(|i| !st.get(&i.id).is_some_and(JobStatus::is_active))
            .filter(|i| !nothing_to_do(&s, &i.run))
            .map(|i| JobSpec {
                id: i.id,
                input: i.path.clone(),
                source_root: i.root.clone(),
                preset: s.preset_by(&i.run.preset_id),
                cleanup: s.cleanup_by(&i.run.cleanup_id),
                destination: s.destination_by(&i.run.destination_id),
                overwrite: i.run.overwrite,
                effects: s.effects_by(&i.run.effects_id),
            })
            .collect();
        drop(st);
        drop(s);
        runner.start(specs, parallel);
    };
    // Writes the panel's choices onto the selected rows (the ones not queued or converting right now).
    let apply = move |_| {
        let run = settings.read().choice();
        let st = statuses.peek();
        let sel = selected.peek();
        for i in items.write().iter_mut() {
            if sel.contains(&i.id) && !st.get(&i.id).is_some_and(JobStatus::is_active) {
                i.run = run.clone();
            }
        }
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
        let left: HashSet<u64> = items.read().iter().map(|i| i.id).collect();
        selected.write().retain(|id| left.contains(id));
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
    // Every file keeps its format, its tags and applies nothing: converting would only re-encode them,
    // so there is nothing to do and the button is off.
    let nothing_to_do = {
        let st = statuses.read();
        !items.read().iter().any(|i| !st.get(&i.id).is_some_and(JobStatus::is_active) && !nothing_to_do(&s, &i.run))
    };
    let cfg = s.clone();
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
    let s_main_apply = t("btnMainApply");
    let s_main_apply_tip = t("tipMainApply");
    let sel_now = selected.read().clone();
    let can_apply = sel_now.iter().any(|id| !statuses.read().get(id).is_some_and(JobStatus::is_active));
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
            engine::run_summary(st.values(), &settings.read().lang)
        } else {
            t("msgMainReady").replace("{n}", &total.to_string())
        }
    };
    let status_text = if !sel_now.is_empty() && !busy {
        let n = t("msgMainSelected").replace("{n}", &sel_now.len().to_string());
        if status_text.is_empty() { n } else { format!("{status_text} · {n}") }
    } else {
        status_text
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
        // ⚠ CLAUDE: the header holds the file actions only (icon-only, tooltips). The choices that shape a
        // run - preset, tags, effects, where it goes, how many at once, Convert - live in the right-hand
        // panel below, one aligned column. A second bar under the header was removed on request.
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
        div { class: "converter-body",
        div { class: "converter-main",
        if ffmpeg_missing {
            div { class: "workspace-empty converter-note", "{no_ffmpeg}" }
        }
        // ⚠ CLAUDE: tabindex makes the list the thing a click focuses, which is what lets Ctrl+A reach it;
        // a click on the empty part clears the selection (a row's own click stops here).
        div {
            class: "lv-scroll converter-list",
            tabindex: "0",
            onclick: move |_| selected.set(HashSet::new()),
            onkeydown: move |e| {
                let m = e.modifiers();
                let ctrl = m.contains(Modifiers::CONTROL) || m.contains(Modifiers::META);
                if ctrl && matches!(e.key(), Key::Character(ref c) if c.eq_ignore_ascii_case("a")) {
                    e.prevent_default();
                    selected.set(items.peek().iter().map(|i| i.id).collect());
                }
            },
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
                    // The row's own choices as tags: the control's icon and the name chosen. What keeps
                    // everything as it is is left out, so a plain row shows only its preset.
                    let r = &item.run;
                    let preset_name = if r.preset_id == engine::preset::ORIGINAL_ID { t("lblMainPresetOriginal") } else { cfg.preset_by(&r.preset_id).name };
                    let mut tags: Vec<(TagKind, String, String)> = vec![(TagKind::Preset, preset_name, s_main_preset.clone())];
                    let cleanup = cfg.cleanup_by(&r.cleanup_id);
                    if cleanup.edits_tags() || cleanup.remove_cover {
                        tags.push((TagKind::Cleanup, cleanup.name.clone(), s_main_cleanup.clone()));
                    }
                    let effects = cfg.effects_by(&r.effects_id);
                    if !effects.items.is_empty() {
                        tags.push((TagKind::Effects, effects.name.clone(), s_main_effects.clone()));
                    }
                    if r.destination_id != SOURCE_ID {
                        tags.push((TagKind::Destination, cfg.destination_by(&r.destination_id).name, s_main_destination.clone()));
                    }
                    match r.overwrite {
                        Overwrite::Skip => {}
                        Overwrite::Overwrite => tags.push((TagKind::Overwrite, s_main_overwrite_overwrite.clone(), s_main_overwrite.clone())),
                        Overwrite::Rename => tags.push((TagKind::Overwrite, s_main_overwrite_rename.clone(), s_main_overwrite.clone())),
                    }
                    let row_class = if sel_now.contains(&id) { "lv-item converter-row active" } else { "lv-item converter-row" };
                    rsx! {
                        div {
                            key: "{id}",
                            class: "{row_class}",
                            onclick: move |e| {
                                e.stop_propagation();
                                let m = e.modifiers();
                                let ctrl = m.contains(Modifiers::CONTROL) || m.contains(Modifiers::META);
                                let ids: Vec<u64> = items.peek().iter().map(|i| i.id).collect();
                                if m.contains(Modifiers::SHIFT) {
                                    let from = (*anchor.peek()).unwrap_or(id);
                                    let mut set = if ctrl { selected.peek().clone() } else { HashSet::new() };
                                    set.extend(range_between(&ids, from, id));
                                    selected.set(set);
                                } else if ctrl {
                                    let mut set = selected.peek().clone();
                                    if !set.remove(&id) {
                                        set.insert(id);
                                    }
                                    selected.set(set);
                                    anchor.set(Some(id));
                                } else {
                                    selected.set(HashSet::from([id]));
                                    anchor.set(Some(id));
                                }
                            },
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
                                div { class: "converter-tags",
                                    for (n, (kind, text, title)) in tags.into_iter().enumerate() {
                                        RunTag { key: "{n}", kind, text, title }
                                    }
                                }
                            }
                            if !badge.is_empty() {
                                span { class: "lv-item-badge converter-state converter-state-{state}", title: "{detail}", "{badge}" }
                            }
                            button {
                                class: "btn icon subtle",
                                title: "{tip}",
                                "aria-label": "{tip}",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    if active {
                                        if let Some(r) = RUNNER.as_ref() {
                                            r.cancel(id);
                                        }
                                    } else {
                                        if let Some(r) = RUNNER.as_ref() {
                                            r.remove(id);
                                        }
                                        items.write().retain(|i| i.id != id);
                                        selected.write().remove(&id);
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
        // The run's choices, one column, every row the same width: a select with the button that edits
        // what it picks, welded the way the header's groups are. Tooltips name them (no labels).
        aside { class: "pnl converter-panel",
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
                    Icon { width: 14, height: 14, icon: LdEraser }
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
                    Icon { width: 14, height: 14, icon: LdAudioWaveform }
                }
            }
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
                    Icon { width: 14, height: 14, icon: LdListMusic }
                }
            }
            div { class: "barh-grp",
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
            }
            // ⚠ CLAUDE: the panel is a brush. It never shows a row's choices and selecting rows never changes
            // it; Apply writes ALL of it onto the selected rows, and is off when none can take it.
            button {
                class: "btn converter-panel-apply",
                title: "{s_main_apply_tip}",
                "aria-label": "{s_main_apply_tip}",
                disabled: !can_apply,
                onclick: apply,
                Icon { width: 14, height: 14, icon: LdListChecks }
                " {s_main_apply}"
            }
            div { class: "converter-panel-run",
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
        }
        div { class: "barh-footer",
            span { class: "converter-status", "{status_text}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::range_between;

    #[test]
    fn a_range_runs_in_list_order_whichever_end_came_first() {
        let ids = [10, 11, 12, 13, 14];
        assert_eq!(range_between(&ids, 11, 13), vec![11, 12, 13]);
        assert_eq!(range_between(&ids, 13, 11), vec![11, 12, 13]);
        assert_eq!(range_between(&ids, 12, 12), vec![12]);
        assert_eq!(range_between(&ids, 99, 12), vec![12], "an anchor no longer in the list selects just the clicked row");
    }
}
