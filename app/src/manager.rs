//! The three editors - presets, destinations and tag cleaning - as pages of the Settings view. Each is a list on
//! the left and the options of the selected entry on the right; every change is stored at once,
//! there is no Save button.
//!
//! ⚠ CLAUDE: NO HARDCODED UI STRINGS. Format names, tag tokens and encoder units are proper
//! nouns / identifiers and stay literal; everything else goes through the t() lookup with a literal
//! key at the call site.

use crate::{
    catalog::{Knob, NumberInput},
    i18n,
    settings::Settings,
};
use dioxus::{html::input_data::MouseButton, prelude::*};
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdCopy, LdFolderOpen, LdPlus, LdTrash2},
};
use engine::{
    CleanupMode, Destination, FlacStereo, Format, Id3Version, OpusApplication, OpusBandwidth, OpusVbr, Preset, QualityKind,
    RateControl, TagCleanup,
    effects::{Channels, ChannelsMode, CutRange, DeEsser, Denoise, Dynamics, DynamicsMode, EQ_BANDS_HZ, Effect, EffectsEntry, Equalizer, Fade, Kind, LimitDuration, LowCut, Repair, ReplayGainTags, SilenceEnd, Tempo, Tone, TrimSilence, Volume, VolumeMode},
    options::{FLAC_BLOCK_SIZES, OPUS_FRAME_TENTHS},
    destination::{self, SOURCE_ID, TOKENS},
};
use std::collections::HashMap;

/// The keys of the Settings tabs this module provides.
pub const PRESETS: &str = "presets";
pub const DESTINATIONS: &str = "destinations";
pub const CLEANUPS: &str = "cleanups";
pub const EFFECTS: &str = "effects";
pub const NOTIFICATIONS: &str = "notifications";

/// Put the dragged entry into the slot of the one it was dragged over. The order of the saved list IS
/// the order everywhere (the lists here and the drop-downs of the window), so this is the whole
/// feature: the editors only decide when to call it.
fn reorder<T>(items: &mut Vec<T>, id_of: impl Fn(&T) -> &str, dragged: &str, target: &str) {
    let from = items.iter().position(|x| id_of(x) == dragged);
    let to = items.iter().position(|x| id_of(x) == target);
    if let (Some(from), Some(to)) = (from, to) {
        if from != to {
            let item = items.remove(from);
            items.insert(to, item);
        }
    }
}

/// Apply `edit` to the preset with this id and re-normalize it, so no field can be left holding a
/// value the new format does not accept.
fn edit_preset(mut settings: Signal<Settings>, id: &str, edit: impl FnOnce(&mut Preset)) {
    let mut s = settings.write();
    if let Some(p) = s.presets.iter_mut().find(|p| p.id == id) {
        edit(p);
        *p = p.clone().normalized();
    }
}

fn edit_cleanup(mut settings: Signal<Settings>, id: &str, edit: impl FnOnce(&mut TagCleanup)) {
    let mut s = settings.write();
    if let Some(c) = s.cleanups.iter_mut().find(|c| c.id == id) {
        edit(c);
    }
}

fn edit_effects(mut settings: Signal<Settings>, id: &str, edit: impl FnOnce(&mut EffectsEntry)) {
    let mut s = settings.write();
    if let Some(e) = s.effects.iter_mut().find(|e| e.id == id) {
        edit(e);
        *e = e.clone().normalized();
    }
}

/// The value of an option in the add menu, and its label.
fn kind_value(k: Kind) -> &'static str {
    match k {
        Kind::TrimSilence => "trim_silence",
        Kind::CutRange => "cut_range",
        Kind::LimitDuration => "limit_duration",
        Kind::Channels => "channels",
        Kind::Repair => "repair",
        Kind::LowCut => "low_cut",
        Kind::Denoise => "denoise",
        Kind::DeEsser => "de_esser",
        Kind::Equalizer => "equalizer",
        Kind::Tone => "tone",
        Kind::Dynamics => "dynamics",
        Kind::Tempo => "tempo",
        Kind::Volume => "volume",
        Kind::FadeIn => "fade_in",
        Kind::FadeOut => "fade_out",
        Kind::SilenceEnd => "silence_end",
        Kind::ReplayGain => "replay_gain",
    }
}

fn kind_from_value(v: &str) -> Option<Kind> {
    Kind::ALL.into_iter().find(|k| kind_value(*k) == v)
}

fn kind_label(k: Kind, lang: &str) -> String {
    match k {
        Kind::TrimSilence => i18n::t(lang, "lblFxTrimSilence"),
        Kind::CutRange => i18n::t(lang, "lblFxCutRange"),
        Kind::LimitDuration => i18n::t(lang, "lblFxLimit"),
        Kind::Channels => i18n::t(lang, "lblFxChannels"),
        Kind::Repair => i18n::t(lang, "lblFxRepair"),
        Kind::LowCut => i18n::t(lang, "lblFxLowCut"),
        Kind::Denoise => i18n::t(lang, "lblFxDenoise"),
        Kind::DeEsser => i18n::t(lang, "lblFxDeEsser"),
        Kind::Equalizer => i18n::t(lang, "lblFxEqualizer"),
        Kind::Tone => i18n::t(lang, "lblFxTone"),
        Kind::Dynamics => i18n::t(lang, "lblFxDynamics"),
        Kind::Tempo => i18n::t(lang, "lblFxTempo"),
        Kind::Volume => i18n::t(lang, "lblFxVolume"),
        Kind::FadeIn => i18n::t(lang, "lblFxFadeIn"),
        Kind::FadeOut => i18n::t(lang, "lblFxFadeOut"),
        Kind::SilenceEnd => i18n::t(lang, "lblFxSilenceEnd"),
        Kind::ReplayGain => i18n::t(lang, "lblFxReplayGain"),
    }
}

/// The editor of one kind's values, found by its kind.
macro_rules! item_editor {
    ($name:ident, $variant:ident, $kind:ident, $ty:ty) => {
        fn $name(settings: Signal<Settings>, id: &str, f: impl FnOnce(&mut $ty)) {
            edit_effects(settings, id, |x| {
                if let Some(Effect::$variant(v)) = x.find_mut(Kind::$kind) {
                    f(v);
                }
            });
        }
    };
}
item_editor!(edit_channels, Channels, Channels, Channels);
item_editor!(edit_repair, Repair, Repair, Repair);
item_editor!(edit_low_cut, LowCut, LowCut, LowCut);
item_editor!(edit_denoise, Denoise, Denoise, Denoise);
item_editor!(edit_de_esser, DeEsser, DeEsser, DeEsser);
item_editor!(edit_equalizer, Equalizer, Equalizer, Equalizer);
item_editor!(edit_tone, Tone, Tone, Tone);
item_editor!(edit_dynamics, Dynamics, Dynamics, Dynamics);
item_editor!(edit_tempo, Tempo, Tempo, Tempo);

fn edit_trim(settings: Signal<Settings>, id: &str, f: impl FnOnce(&mut TrimSilence)) {
    edit_effects(settings, id, |x| {
        if let Some(Effect::TrimSilence(t)) = x.find_mut(Kind::TrimSilence) {
            f(t);
        }
    });
}

fn edit_cut(settings: Signal<Settings>, id: &str, f: impl FnOnce(&mut CutRange)) {
    edit_effects(settings, id, |x| {
        if let Some(Effect::CutRange(c)) = x.find_mut(Kind::CutRange) {
            f(c);
        }
    });
}

fn edit_limit(settings: Signal<Settings>, id: &str, f: impl FnOnce(&mut LimitDuration)) {
    edit_effects(settings, id, |x| {
        if let Some(Effect::LimitDuration(l)) = x.find_mut(Kind::LimitDuration) {
            f(l);
        }
    });
}

fn edit_volume(settings: Signal<Settings>, id: &str, f: impl FnOnce(&mut Volume)) {
    edit_effects(settings, id, |x| {
        if let Some(Effect::Volume(v)) = x.find_mut(Kind::Volume) {
            f(v);
        }
    });
}

fn edit_fade(settings: Signal<Settings>, id: &str, kind: Kind, f: impl FnOnce(&mut Fade)) {
    edit_effects(settings, id, |x| match x.find_mut(kind) {
        Some(Effect::FadeIn(fd)) | Some(Effect::FadeOut(fd)) => f(fd),
        _ => {}
    });
}

fn edit_silence(settings: Signal<Settings>, id: &str, f: impl FnOnce(&mut SilenceEnd)) {
    edit_effects(settings, id, |x| {
        if let Some(Effect::SilenceEnd(s)) = x.find_mut(Kind::SilenceEnd) {
            f(s);
        }
    });
}

fn edit_replaygain(settings: Signal<Settings>, id: &str, f: impl FnOnce(&mut ReplayGainTags)) {
    edit_effects(settings, id, |x| {
        if let Some(Effect::ReplayGain(r)) = x.find_mut(Kind::ReplayGain) {
            f(r);
        }
    });
}

fn edit_destination(mut settings: Signal<Settings>, id: &str, edit: impl FnOnce(&mut Destination)) {
    let mut s = settings.write();
    if let Some(d) = s.destinations.iter_mut().find(|d| d.id == id) {
        edit(d);
    }
}

/// The exceptions to the cleanup's mode: one name per line, or several separated by `;`. The text is
/// stored as typed, line by line, and split at the `;` only when a conversion uses it. It keeps the text as typed (the cleanup stores the
/// tidied list), otherwise the blank line a user is about to fill would vanish on every keystroke.
/// The parent gives it a `key` of the cleanup id, so it starts over when another one is picked.
///
/// Under the box, the known field names that fit the line being typed (the one the caret is on) are
/// offered as chips; a click puts the name in place of what was typed.
#[component]
fn ExceptionsField(settings: Signal<Settings>, id: String, lang: String, mode: CleanupMode) -> Element {
    let initial = settings
        .read()
        .cleanups
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.exceptions.join(&char::from(10).to_string()))
        .unwrap_or_default();
    let mut raw = use_signal(|| initial);
    // The caret as a position in the text (UTF-16 units, as the box reports it); the end until known.
    let mut caret = use_signal(|| usize::MAX);
    let t = |key: &str| i18n::t(&lang, key);
    // With "keep all" the list names what is deleted; with "delete all" it names what is kept.
    let label = match mode {
        CleanupMode::Keep => t("lblClnRemoveTags"),
        CleanupMode::Delete => t("lblClnKeepTags"),
    };
    let hint = t("msgClnRemoveTagsHint");
    let box_id = format!("remove-tags-{id}");

    // Ask the box where the caret is. Typing, clicking and the arrow keys all move it.
    let track = {
        let box_id = box_id.clone();
        move || {
            let box_id = box_id.clone();
            spawn(async move {
                let mut ev = document::eval(&format!(
                    "dioxus.send(document.getElementById('{box_id}')?.selectionStart ?? -1);"
                ));
                if let Ok(pos) = ev.recv::<i64>().await {
                    caret.set(if pos < 0 { usize::MAX } else { pos as usize });
                }
            });
        }
    };

    let text = raw();
    let (start, end) = entry_span(&text, caret());
    let typed = &text[start..end];
    let taken: Vec<String> = text[..start].split(is_separator).chain(text[end..].split(is_separator)).map(str::to_owned).collect();
    let suggestions = engine::tags::suggest_names(typed, &taken, 8);

    let id_ev = id.clone();
    let oninput_track = track.clone();
    let onkeyup_track = track.clone();
    let onclick_track = track;
    rsx! {
        div { class: "dlg-field",
            label { class: "dlg-field-label", "{label}" }
            textarea {
                id: "{box_id}",
                class: "textarea",
                rows: "5",
                spellcheck: "false",
                value: "{raw}",
                oninput: move |e| {
                    let text = e.value();
                    let list: Vec<String> = text.lines().map(str::to_owned).collect();
                    raw.set(text);
                    edit_cleanup(settings, &id_ev, |c| c.exceptions = list);
                    oninput_track();
                },
                onkeyup: move |_| onkeyup_track(),
                onclick: move |_| onclick_track(),
            }
            if !suggestions.is_empty() {
                div { class: "manager-suggest",
                    for name in suggestions {
                        {
                            let box_id = box_id.clone();
                            let id = id.clone();
                            rsx! {
                                button {
                                    key: "{name}",
                                    class: "chip",
                                    onclick: move |_| {
                                        let (new_text, end) = replace_span(&raw(), (start, end), name);
                                        let list: Vec<String> = new_text.lines().map(str::to_owned).collect();
                                        raw.set(new_text);
                                        caret.set(end);
                                        edit_cleanup(settings, &id, |c| c.exceptions = list);
                                        // Back into the box, caret at the end of the name just placed.
                                        let _ = document::eval(&format!(
                                            "const b = document.getElementById('{box_id}'); if (b) {{ b.focus(); b.setSelectionRange({end}, {end}); }}"
                                        ));
                                    },
                                    "{name}"
                                }
                            }
                        }
                    }
                }
            }
            p { class: "hint", "{hint}" }
        }
    }
}

/// A name ends at a line break or at a `;`.
fn is_separator(c: char) -> bool {
    c == '\n' || c == ';'
}

/// The byte range of the name a caret at `pos` (UTF-16 units; `usize::MAX` = at the end) is in: from
/// after the previous separator to the next one.
fn entry_span(text: &str, pos: usize) -> (usize, usize) {
    let mut units = 0usize;
    let mut at = text.len();
    for (i, ch) in text.char_indices() {
        if units >= pos {
            at = i;
            break;
        }
        units += ch.len_utf16();
    }
    let start = text[..at].rfind(is_separator).map_or(0, |i| i + 1);
    let end = text[at..].find(is_separator).map_or(text.len(), |i| at + i);
    (start, end)
}

/// `text` with the byte range `span` replaced by `name`, and the caret position (UTF-16 units) at the
/// end of that name.
fn replace_span(text: &str, span: (usize, usize), name: &str) -> (String, usize) {
    let (start, end) = span;
    let caret = text[..start].encode_utf16().count() + name.encode_utf16().count();
    (format!("{}{}{}", &text[..start], name, &text[end..]), caret)
}

#[component]
pub fn PresetEditor(settings: Signal<Settings>) -> Element {
    let mut selected = use_signal(|| settings.read().preset_id.clone());

    let lang = settings.read().lang.clone();
    let t = |key: &str| i18n::t(&lang, key);

    // The selection can point at a preset that was just deleted; fall back to the first.
    let presets = settings.read().presets.clone();
    // The list is in the saved order, which the user changes by dragging.
    let listed = presets.clone();
    let mut dragging = use_signal(|| Option::<String>::None);
    let s_reorder = t("msgMngReorderHint");
    let current = presets.iter().find(|p| p.id == selected()).unwrap_or(&presets[0]).clone();
    let cur_id = current.id.clone();
    let can_delete = presets.len() > 1;

    let s_new = t("btnMngNew");
    let s_dup = t("btnMngDuplicate");
    let s_del = t("btnMngDelete");
    let s_name = t("lblPrsName");
    let s_format = t("lblPrsFormat");
    let s_rate_control = t("lblPrsRateControl");
    let s_cbr = t("lblPrsRateCbr");
    let s_vbr = t("lblPrsRateVbr");
    let s_abr = t("lblPrsRateAbr");
    let s_sample_rate = t("lblPrsSampleRate");
    let s_channels = t("lblPrsChannels");
    let s_bit_depth = t("lblPrsBitDepth");
    let s_keep = t("lblPrsKeep");
    let s_mono = t("lblPrsMono");
    let s_stereo = t("lblPrsStereo");
    let s_ringtone = t("msgPrsRingtoneLimit");
    let s_id3 = t("lblPrsId3Version");
    let s_id3_hint = t("msgPrsId3Hint");
    let knob_label = match current.quality_kind() {
        QualityKind::Bitrate => t("lblPrsQualityBitrate"),
        QualityKind::VbrLevel => t("lblPrsQualityVbr"),
        QualityKind::Compression => t("lblPrsQualityCompression"),
        QualityKind::None => String::new(),
    };

    let id_for_name = cur_id.clone();
    let id_for_format = cur_id.clone();
    let id_for_rate = cur_id.clone();
    let id_for_knob = cur_id.clone();
    let id_for_sr = cur_id.clone();
    let id_for_ch = cur_id.clone();
    let id_for_bd = cur_id.clone();
    let id_for_dither = cur_id.clone();
    let id_for_id3 = cur_id.clone();
    let id_for_dup = cur_id.clone();
    let id_for_del = cur_id.clone();

    let sample_rates = current.sample_rates();
    let bit_depths = current.bit_depths();
    let knob = current.knob_range();

    rsx! {
        div { class: "manager",
            div { class: "manager-list",
                div { class: "manager-actions",
                    button {
                        class: "btn icon",
                        title: "{s_new}",
                        "aria-label": "{s_new}",
                        onclick: move |_| {
                            let name = s_new.clone();
                            let p = Preset::new(&name, Format::Mp3);
                            selected.set(p.id.clone());
                            settings.write().presets.push(p);
                        },
                        Icon { width: 14, height: 14, icon: LdPlus }
                    }
                    button {
                        class: "btn icon",
                        title: "{s_dup}",
                        "aria-label": "{s_dup}",
                        onclick: move |_| {
                            let src = settings.read().presets.iter().find(|p| p.id == id_for_dup).cloned();
                            if let Some(mut p) = src {
                                p.id = engine::preset::new_id();
                                p.name = format!("{} 2", p.name);
                                selected.set(p.id.clone());
                                settings.write().presets.push(p);
                            }
                        },
                        Icon { width: 14, height: 14, icon: LdCopy }
                    }
                    button {
                        class: "btn icon",
                        title: "{s_del}",
                        "aria-label": "{s_del}",
                        disabled: !can_delete,
                        onclick: move |_| {
                            settings.write().presets.retain(|p| p.id != id_for_del);
                            let fixed = settings.read().clone().repaired();
                            selected.set(fixed.presets[0].id.clone());
                            settings.set(fixed);
                        },
                        Icon { width: 14, height: 14, icon: LdTrash2 }
                    }
                }
                div { class: "lv-scroll",
                    for p in listed.iter() {
                        {
                            let id = p.id.clone();
                            let id_down = p.id.clone();
                            let id_over = p.id.clone();
                            let active = p.id == cur_id;
                            let moving = dragging().as_deref() == Some(p.id.as_str());
                            rsx! {
                                button {
                                    key: "{p.id}",
                                    class: match (active, moving) {
                                        (_, true) => "lv-item reordering",
                                        (true, _) => "lv-item active",
                                        _ => "lv-item",
                                    },
                                    onpointerdown: move |e| {
                                        if e.trigger_button() == Some(MouseButton::Primary) {
                                            dragging.set(Some(id_down.clone()));
                                        }
                                    },
                                    onpointerenter: move |e| {
                                        let Some(from) = dragging() else { return };
                                        if !e.held_buttons().contains(MouseButton::Primary) {
                                            dragging.set(None);
                                            return;
                                        }
                                        reorder(&mut settings.write().presets, |p| p.id.as_str(), &from, &id_over);
                                    },
                                    onpointerup: move |_| dragging.set(None),
                                    onclick: move |_| {
                                        selected.set(id.clone());
                                        settings.write().preset_id = id.clone();
                                    },
                                    div { class: "lv-item-body",
                                        div { class: "lv-item-name", "{p.name}" }
                                    }
                                    span { class: "lv-item-badge", "{p.format.label()}" }
                                }
                            }
                        }
                    }
                }
                p { class: "hint", "{s_reorder}" }
            }
            div { class: "manager-form",
                div { class: "dlg-field",
                    label { class: "dlg-field-label", "{s_name}" }
                    input {
                        class: "input",
                        value: "{current.name}",
                        oninput: move |e| {
                            let v = e.value();
                            edit_preset(settings, &id_for_name, |p| p.name = v);
                        },
                    }
                }
                div { class: "dlg-field",
                    label { class: "dlg-field-label", "{s_format}" }
                    select {
                        class: "select",
                        value: "{current.format.id()}",
                        onchange: move |e| {
                            if let Some(f) = Format::from_id(&e.value()) {
                                edit_preset(settings, &id_for_format, |p| p.format = f);
                            }
                        },
                        for f in Format::ALL {
                            option { key: "{f.id()}", value: "{f.id()}", selected: f == current.format, "{f.label()}" }
                        }
                    }
                }
                if current.format == Format::M4r {
                    p { class: "hint", "{s_ringtone}" }
                }
                if current.supports_vbr() {
                    div { class: "dlg-field",
                        label { class: "dlg-field-label", "{s_rate_control}" }
                        select {
                            class: "select",
                            value: match current.rate_control {
                                RateControl::Vbr => "vbr",
                                RateControl::Abr => "abr",
                                RateControl::Cbr => "cbr",
                            },
                            onchange: move |e| {
                                let rc = match e.value().as_str() {
                                    "vbr" => RateControl::Vbr,
                                    "abr" => RateControl::Abr,
                                    _ => RateControl::Cbr,
                                };
                                edit_preset(settings, &id_for_rate, |p| p.rate_control = rc);
                            },
                            option { value: "cbr", selected: current.rate_control == RateControl::Cbr, "{s_cbr}" }
                            option { value: "vbr", selected: current.rate_control == RateControl::Vbr, "{s_vbr}" }
                            if current.supports_abr() {
                                option { value: "abr", selected: current.rate_control == RateControl::Abr, "{s_abr}" }
                            }
                        }
                    }
                }
                if let Some(r) = knob {
                    div { class: "dlg-field",
                        Knob {
                            // The key makes the control remount when its meaning changes (bitrate
                            // to a quality level), so it never shows a value in the wrong scale.
                            key: "{current.format.id()}-{current.quality_kind() as u8}",
                            label: knob_label,
                            value: current.quality_value(),
                            min: r.min,
                            max: r.max,
                            step: r.step,
                            unit: r.unit.to_owned(),
                            on_change: move |v: f64| edit_preset(settings, &id_for_knob, |p| p.set_quality(v)),
                        }
                    }
                }
                div { class: "dlg-field",
                    label { class: "dlg-field-label", "{s_sample_rate}" }
                    select {
                        class: "select",
                        value: "{current.sample_rate.map(|r| r.to_string()).unwrap_or_default()}",
                        onchange: move |e| {
                            let v = e.value().parse::<u32>().ok();
                            edit_preset(settings, &id_for_sr, |p| p.sample_rate = v);
                        },
                        option { value: "", selected: current.sample_rate.is_none(), "{s_keep}" }
                        for r in sample_rates.iter().copied() {
                            option { key: "{r}", value: "{r}", selected: current.sample_rate == Some(r), "{r} Hz" }
                        }
                    }
                }
                div { class: "dlg-field",
                    label { class: "dlg-field-label", "{s_channels}" }
                    select {
                        class: "select",
                        value: "{current.channels.map(|c| c.to_string()).unwrap_or_default()}",
                        onchange: move |e| {
                            let v = e.value().parse::<u8>().ok();
                            edit_preset(settings, &id_for_ch, |p| p.channels = v);
                        },
                        option { value: "", selected: current.channels.is_none(), "{s_keep}" }
                        option { value: "1", selected: current.channels == Some(1), "{s_mono}" }
                        option { value: "2", selected: current.channels == Some(2), "{s_stereo}" }
                    }
                }
                if current.supports_bit_depth() {
                    div { class: "dlg-field",
                        label { class: "dlg-field-label", "{s_bit_depth}" }
                        select {
                            class: "select",
                            value: "{current.bit_depth.unwrap_or(16)}",
                            onchange: move |e| {
                                let v = e.value().parse::<u8>().ok();
                                edit_preset(settings, &id_for_bd, |p| p.bit_depth = v);
                            },
                            for d in bit_depths.iter().copied() {
                                option { key: "{d}", value: "{d}", selected: current.bit_depth.unwrap_or(16) == d, "{d} bit" }
                            }
                        }
                    }
                }
                if current.is_16_bit() {
                    ToggleField {
                        label: t("lblPrsDither"),
                        checked: current.dither,
                        on_change: {
                            move |v| edit_preset(settings, &id_for_dither, |p| p.dither = v)
                        },
                    }
                }
                if current.supports_id3_version() {
                    div { class: "dlg-field",
                        label { class: "dlg-field-label", "{s_id3}" }
                        select {
                            class: "select",
                            value: match current.id3_version { Id3Version::V23 => "v23", Id3Version::V24 => "v24" },
                            onchange: move |e| {
                                let v = if e.value() == "v24" { Id3Version::V24 } else { Id3Version::V23 };
                                edit_preset(settings, &id_for_id3, |p| p.id3_version = v);
                            },
                            option { value: "v23", selected: current.id3_version == Id3Version::V23, "ID3v2.3" }
                            option { value: "v24", selected: current.id3_version == Id3Version::V24, "ID3v2.4" }
                        }
                        p { class: "hint", "{s_id3_hint}" }
                    }
                }
                AdvancedOptions { settings, id: cur_id.clone(), lang: lang.clone() }
            }
        }
    }
}

#[component]
pub fn DestinationEditor(settings: Signal<Settings>) -> Element {
    let mut selected = use_signal(|| settings.read().destination_id.clone());

    let lang = settings.read().lang.clone();
    let t = |key: &str| i18n::t(&lang, key);

    let dests = settings.read().destinations.clone();
    let mut dragging = use_signal(|| Option::<String>::None);
    let s_reorder = t("msgMngReorderHint");
    // The built-in entry is always first and is what an unknown selection falls back to.
    let is_source = selected() == SOURCE_ID || !dests.iter().any(|d| d.id == selected());
    let current = dests.iter().find(|d| d.id == selected()).cloned();
    let cur_id = current.as_ref().map(|d| d.id.clone()).unwrap_or_default();

    let s_new = t("btnMngNew");
    let s_dup = t("btnMngDuplicate");
    let s_del = t("btnMngDelete");
    let s_name = t("lblDstName");
    let s_folder = t("lblDstFolder");
    let s_browse = t("btnDstBrowse");
    let s_pattern = t("lblDstPattern");
    let s_hint = t("msgDstPatternHint");
    let s_preview = t("lblDstPreview");
    let s_source_label = t("lblMainDestSource");
    let s_source_fixed = t("msgDstSourceFixed");
    let s_no_folder = t("msgDstNoFolder");
    let s_keep_structure = t("lblDstKeepStructure");

    // The example uses the format of the preset currently picked in the window.
    // "(Original)" has no format of its own: the example shows a placeholder extension.
    let ext = {
        let p = settings.read().selected_preset();
        if p.is_original() { "*" } else { p.format.extension() }
    };
    let sample: HashMap<String, String> = [
        ("artist", "Artist"),
        ("album_artist", "Artist"),
        ("album", "Album"),
        ("title", "Title"),
        ("track", "3"),
        ("disc", "1"),
        ("date", "2024"),
        ("genre", "Genre"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();

    let id_name = cur_id.clone();
    let id_folder = cur_id.clone();
    let id_browse = cur_id.clone();
    let id_pattern = cur_id.clone();
    let id_keep = cur_id.clone();
    let id_dup = cur_id.clone();
    let id_del = cur_id.clone();

    rsx! {
        div { class: "manager",
            div { class: "manager-list",
                div { class: "manager-actions",
                    button {
                        class: "btn icon",
                        title: "{s_new}",
                        "aria-label": "{s_new}",
                        onclick: move |_| {
                            let folder = dirs::audio_dir().map(|p| p.display().to_string()).unwrap_or_default();
                            let d = Destination::new(&s_new, &folder);
                            selected.set(d.id.clone());
                            settings.write().destinations.push(d);
                        },
                        Icon { width: 14, height: 14, icon: LdPlus }
                    }
                    button {
                        class: "btn icon",
                        title: "{s_dup}",
                        "aria-label": "{s_dup}",
                        disabled: is_source,
                        onclick: move |_| {
                            let src = settings.read().destinations.iter().find(|d| d.id == id_dup).cloned();
                            if let Some(mut d) = src {
                                d.id = engine::preset::new_id();
                                d.name = format!("{} 2", d.name);
                                selected.set(d.id.clone());
                                settings.write().destinations.push(d);
                            }
                        },
                        Icon { width: 14, height: 14, icon: LdCopy }
                    }
                    button {
                        class: "btn icon",
                        title: "{s_del}",
                        "aria-label": "{s_del}",
                        disabled: is_source,
                        onclick: move |_| {
                            settings.write().destinations.retain(|d| d.id != id_del);
                            let fixed = settings.read().clone().repaired();
                            settings.set(fixed);
                            selected.set(SOURCE_ID.to_owned());
                        },
                        Icon { width: 14, height: 14, icon: LdTrash2 }
                    }
                }
                div { class: "lv-scroll",
                    button {
                        class: if is_source { "lv-item active" } else { "lv-item" },
                        onclick: move |_| {
                            selected.set(SOURCE_ID.to_owned());
                            settings.write().destination_id = SOURCE_ID.to_owned();
                        },
                        div { class: "lv-item-body",
                            div { class: "lv-item-name", "{s_source_label}" }
                        }
                    }
                    for d in dests.iter() {
                        {
                            let id = d.id.clone();
                            let id_down = d.id.clone();
                            let id_over = d.id.clone();
                            let active = !is_source && d.id == cur_id;
                            let moving = dragging().as_deref() == Some(d.id.as_str());
                            rsx! {
                                button {
                                    key: "{d.id}",
                                    class: match (active, moving) {
                                        (_, true) => "lv-item reordering",
                                        (true, _) => "lv-item active",
                                        _ => "lv-item",
                                    },
                                    onpointerdown: move |e| {
                                        if e.trigger_button() == Some(MouseButton::Primary) {
                                            dragging.set(Some(id_down.clone()));
                                        }
                                    },
                                    onpointerenter: move |e| {
                                        let Some(from) = dragging() else { return };
                                        if !e.held_buttons().contains(MouseButton::Primary) {
                                            dragging.set(None);
                                            return;
                                        }
                                        reorder(&mut settings.write().destinations, |d| d.id.as_str(), &from, &id_over);
                                    },
                                    onpointerup: move |_| dragging.set(None),
                                    onclick: move |_| {
                                        selected.set(id.clone());
                                        settings.write().destination_id = id.clone();
                                    },
                                    div { class: "lv-item-body",
                                        div { class: "lv-item-name", "{d.name}" }
                                        div { class: "lv-item-sub", "{d.folder}" }
                                    }
                                }
                            }
                        }
                    }
                }
                p { class: "hint", "{s_reorder}" }
            }
            div { class: "manager-form",
                if let Some(d) = current {
                    {
                        let example = {
                            let rel = destination::render_pattern(&d.pattern, &sample, "source-file");
                            let base = d.folder.trim().trim_end_matches(['/', '\\']).to_owned();
                            if base.is_empty() { format!("{rel}.{ext}") } else { format!("{base}/{rel}.{ext}") }
                        };
                        let folder_empty = d.folder.trim().is_empty();
                        let pattern_now = d.pattern.clone();
                        rsx! {
                            div { class: "dlg-field",
                                label { class: "dlg-field-label", "{s_name}" }
                                input {
                                    class: "input",
                                    value: "{d.name}",
                                    oninput: move |e| {
                                        let v = e.value();
                                        edit_destination(settings, &id_name, |x| x.name = v);
                                    },
                                }
                            }
                            div { class: "dlg-field",
                                label { class: "dlg-field-label", "{s_folder}" }
                                div { class: "manager-row",
                                    input {
                                        class: "input",
                                        value: "{d.folder}",
                                        oninput: move |e| {
                                            let v = e.value();
                                            edit_destination(settings, &id_folder, |x| x.folder = v);
                                        },
                                    }
                                    button {
                                        class: "btn",
                                        onclick: move |_| {
                                            let id = id_browse.clone();
                                            spawn(async move {
                                                if let Some(dir) = rfd::AsyncFileDialog::new().pick_folder().await {
                                                    let v = dir.path().display().to_string();
                                                    edit_destination(settings, &id, |x| x.folder = v);
                                                }
                                            });
                                        },
                                        Icon { width: 14, height: 14, icon: LdFolderOpen }
                                        "{s_browse}"
                                    }
                                }
                                if folder_empty {
                                    p { class: "hint warn", "{s_no_folder}" }
                                }
                            }
                            div { class: "dlg-field",
                                label { class: "check-label",
                                    input {
                                        class: "check-box",
                                        r#type: "checkbox",
                                        checked: d.keep_structure,
                                        onchange: move |e| {
                                            let v = e.checked();
                                            edit_destination(settings, &id_keep, |x| x.keep_structure = v);
                                        },
                                    }
                                    "{s_keep_structure}"
                                }
                            }
                            div { class: "dlg-field",
                                label { class: "dlg-field-label", "{s_pattern}" }
                                input {
                                    class: "input",
                                    value: "{d.pattern}",
                                    oninput: move |e| {
                                        let v = e.value();
                                        edit_destination(settings, &id_pattern, |x| x.pattern = v);
                                    },
                                }
                                div { class: "manager-tokens",
                                    for token in TOKENS {
                                        {
                                            let id = cur_id.clone();
                                            let base = pattern_now.clone();
                                            rsx! {
                                                button {
                                                    key: "{token}",
                                                    class: "chip",
                                                    onclick: move |_| {
                                                        let next = format!("{base}{{{token}}}");
                                                        edit_destination(settings, &id, |x| x.pattern = next);
                                                    },
                                                    "{token}"
                                                }
                                            }
                                        }
                                    }
                                }
                                p { class: "hint", "{s_hint}" }
                            }
                            div { class: "dlg-field",
                                label { class: "dlg-field-label", "{s_preview}" }
                                div { class: "manager-example", "{example}" }
                            }
                        }
                    }
                } else {
                    p { class: "hint", "{s_source_fixed}" }
                }
            }
        }
    }
}

// ── the advanced section of a preset ────────────────────────────────────────────────────────────
// Each control is one field of `EncoderOptions`; an empty field means "the encoder decides". Numbers
// use the catalog's NumberField (themed steppers, clamped on blur) at a width that fits a few digits.

/// A labelled catalog number field. The width is for four or five digits plus the steppers.
#[component]
fn NumberField(label: String, value: Option<u32>, min: u32, max: u32, auto: String, on_change: EventHandler<Option<u32>>) -> Element {
    rsx! {
        div { class: "dlg-field",
            label { class: "dlg-field-label", "{label}" }
            NumberInput {
                value,
                min,
                max,
                width: 140,
                placeholder: auto,
                allow_empty: true,
                on_change: move |v| on_change.call(v),
            }
        }
    }
}

/// A whole number that may be negative (a gain, a change of tempo). The catalog's number field is
/// unsigned, so this is the catalog's plain input: a gap the catalog does not fill yet.
#[component]
fn SignedField(label: String, value: i32, min: i32, max: i32, on_change: EventHandler<i32>) -> Element {
    rsx! {
        div { class: "dlg-field",
            label { class: "dlg-field-label", "{label}" }
            input {
                class: "input",
                r#type: "number",
                value: "{value}",
                min: "{min}",
                max: "{max}",
                step: "1",
                oninput: move |e| {
                    if let Ok(n) = e.value().trim().parse::<i32>() {
                        on_change.call(n.clamp(min, max));
                    }
                },
            }
        }
    }
}

#[component]
fn ToggleField(label: String, checked: bool, on_change: EventHandler<bool>) -> Element {
    rsx! {
        div { class: "dlg-field",
            label { class: "check-label",
                input {
                    class: "check-box",
                    r#type: "checkbox",
                    checked,
                    onchange: move |e| on_change.call(e.checked()),
                }
                "{label}"
            }
        }
    }
}

/// A choice from a short list; `""` is the first, "automatic" entry when `auto` is given.
#[component]
fn ChoiceField(label: String, value: String, options: Vec<(String, String)>, on_change: EventHandler<String>) -> Element {
    rsx! {
        div { class: "dlg-field",
            label { class: "dlg-field-label", "{label}" }
            select {
                class: "select",
                value: "{value}",
                onchange: move |e| on_change.call(e.value()),
                for (id, text) in options {
                    option { key: "{id}", value: "{id}", selected: id == value, "{text}" }
                }
            }
        }
    }
}

#[component]
fn AdvancedOptions(settings: Signal<Settings>, id: String, lang: String) -> Element {
    let t = |key: &str| i18n::t(&lang, key);
    let Some(p) = settings.read().presets.iter().find(|p| p.id == id).cloned() else {
        return rsx! {};
    };
    let o = p.options.clone();
    let has_any = p.format == Format::Mp3
        || p.format == Format::Opus
        || p.format == Format::Ogg && p.rate_control != RateControl::Vbr
        || p.is_flac()
        || p.is_aac();
    if !has_any {
        return rsx! {};
    }
    let auto = t("lblAdvAuto");
    let s_adv = t("ttlAdvanced");
    let edit = move |id: &str, f: &dyn Fn(&mut Preset)| edit_preset(settings, id, |p| f(p));

    let bool_choice = |v: Option<bool>| match v {
        None => "",
        Some(true) => "1",
        Some(false) => "0",
    };

    rsx! {
        details { class: "manager-advanced",
            summary { class: "dlg-field-label", "{s_adv}" }
            div { class: "manager-advanced-body",

            if p.format == Format::Mp3 {
                ChoiceField {
                    label: t("lblAdvJointStereo"),
                    value: bool_choice(o.mp3_joint_stereo).to_owned(),
                    options: vec![
                        (String::new(), auto.clone()),
                        ("1".to_owned(), t("lblAdvYes")),
                        ("0".to_owned(), t("lblAdvNo")),
                    ],
                    on_change: {
                        let id = id.clone();
                        move |v: String| {
                            let j = match v.as_str() { "1" => Some(true), "0" => Some(false), _ => None };
                            edit(&id, &|p| p.options.mp3_joint_stereo = j);
                        }
                    },
                }
                NumberField {
                    label: t("lblAdvAlgorithm"),
                    value: o.mp3_algorithm.map(u32::from),
                    min: 0,
                    max: 9,
                    auto: auto.clone(),
                    on_change: {
                        let id = id.clone();
                        move |v: Option<u32>| edit(&id, &|p| p.options.mp3_algorithm = v.map(|x| x.min(9) as u8))
                    },
                }
                NumberField {
                    label: t("lblAdvLowpass"),
                    value: o.mp3_lowpass_hz,
                    min: 1000,
                    max: 22050,
                    auto: auto.clone(),
                    on_change: {
                        let id = id.clone();
                        move |v| edit(&id, &|p| p.options.mp3_lowpass_hz = v)
                    },
                }
                NumberField {
                    label: t("lblAdvHighpass"),
                    value: o.mp3_highpass_hz,
                    min: 10,
                    max: 10000,
                    auto: auto.clone(),
                    on_change: {
                        let id = id.clone();
                        move |v| edit(&id, &|p| p.options.mp3_highpass_hz = v)
                    },
                }
                ToggleField {
                    label: t("lblAdvReservoir"),
                    checked: o.mp3_reservoir,
                    on_change: {
                        let id = id.clone();
                        move |v| edit(&id, &|p| p.options.mp3_reservoir = v)
                    },
                }
                ToggleField {
                    label: t("lblAdvCopyright"),
                    checked: o.mp3_copyright,
                    on_change: {
                        let id = id.clone();
                        move |v| edit(&id, &|p| p.options.mp3_copyright = v)
                    },
                }
                ToggleField {
                    label: t("lblAdvOriginal"),
                    checked: o.mp3_original,
                    on_change: {
                        let id = id.clone();
                        move |v| edit(&id, &|p| p.options.mp3_original = v)
                    },
                }
            }

            if p.format == Format::Ogg && p.rate_control != RateControl::Vbr {
                NumberField {
                    label: t("lblAdvOggMin"),
                    value: o.ogg_min_kbps,
                    min: 32,
                    max: 500,
                    auto: auto.clone(),
                    on_change: {
                        let id = id.clone();
                        move |v| edit(&id, &|p| p.options.ogg_min_kbps = v)
                    },
                }
                NumberField {
                    label: t("lblAdvOggMax"),
                    value: o.ogg_max_kbps,
                    min: 32,
                    max: 500,
                    auto: auto.clone(),
                    on_change: {
                        let id = id.clone();
                        move |v| edit(&id, &|p| p.options.ogg_max_kbps = v)
                    },
                }
            }

            if p.format == Format::Opus {
                ChoiceField {
                    label: t("lblAdvOpusApplication"),
                    value: o.opus_application.ffmpeg().to_owned(),
                    options: vec![
                        ("audio".to_owned(), t("lblAdvOpusMusic")),
                        ("voip".to_owned(), t("lblAdvOpusVoice")),
                        ("lowdelay".to_owned(), t("lblAdvOpusLowDelay")),
                    ],
                    on_change: {
                        let id = id.clone();
                        move |v: String| {
                            let a = match v.as_str() {
                                "voip" => OpusApplication::Voip,
                                "lowdelay" => OpusApplication::LowDelay,
                                _ => OpusApplication::Audio,
                            };
                            edit(&id, &|p| p.options.opus_application = a);
                        }
                    },
                }
                ChoiceField {
                    label: t("lblAdvOpusVbr"),
                    value: o.opus_vbr.ffmpeg().to_owned(),
                    options: vec![
                        ("on".to_owned(), t("lblAdvOpusVbrOn")),
                        ("constrained".to_owned(), t("lblAdvOpusVbrConstrained")),
                        ("off".to_owned(), t("lblAdvOpusVbrOff")),
                    ],
                    on_change: {
                        let id = id.clone();
                        move |v: String| {
                            let m = match v.as_str() {
                                "constrained" => OpusVbr::Constrained,
                                "off" => OpusVbr::Off,
                                _ => OpusVbr::On,
                            };
                            edit(&id, &|p| p.options.opus_vbr = m);
                        }
                    },
                }
                ChoiceField {
                    label: t("lblAdvOpusFrame"),
                    value: o.opus_frame_tenths.map(|v| v.to_string()).unwrap_or_default(),
                    options: std::iter::once((String::new(), auto.clone()))
                        .chain(OPUS_FRAME_TENTHS.iter().map(|t| (t.to_string(), format!("{} ms", f64::from(*t) / 10.0))))
                        .collect(),
                    on_change: {
                        let id = id.clone();
                        move |v: String| edit(&id, &|p| p.options.opus_frame_tenths = v.parse::<u32>().ok())
                    },
                }
                NumberField {
                    label: t("lblAdvOpusComplexity"),
                    value: o.opus_complexity.map(u32::from),
                    min: 0,
                    max: 10,
                    auto: auto.clone(),
                    on_change: {
                        let id = id.clone();
                        move |v: Option<u32>| edit(&id, &|p| p.options.opus_complexity = v.map(|x| x.min(10) as u8))
                    },
                }
                ChoiceField {
                    label: t("lblAdvOpusBandwidth"),
                    value: o.opus_bandwidth.map(|b| b.id().to_owned()).unwrap_or_default(),
                    options: vec![
                        (String::new(), auto.clone()),
                        ("narrow".to_owned(), t("lblAdvOpusNarrow")),
                        ("medium".to_owned(), t("lblAdvOpusMedium")),
                        ("wide".to_owned(), t("lblAdvOpusWide")),
                        ("superwide".to_owned(), t("lblAdvOpusSuperWide")),
                        ("full".to_owned(), t("lblAdvOpusFull")),
                    ],
                    on_change: {
                        let id = id.clone();
                        move |v: String| edit(&id, &|p| p.options.opus_bandwidth = OpusBandwidth::from_id(&v))
                    },
                }
                ToggleField {
                    label: t("lblAdvOpusPhase"),
                    checked: o.opus_phase_inversion,
                    on_change: {
                        let id = id.clone();
                        move |v| edit(&id, &|p| p.options.opus_phase_inversion = v)
                    },
                }
            }

            if p.is_flac() {
                ChoiceField {
                    label: t("lblAdvFlacBlock"),
                    value: o.flac_block_size.map(|v| v.to_string()).unwrap_or_default(),
                    options: std::iter::once((String::new(), auto.clone()))
                        .chain(FLAC_BLOCK_SIZES.iter().map(|b| (b.to_string(), b.to_string())))
                        .collect(),
                    on_change: {
                        let id = id.clone();
                        move |v: String| edit(&id, &|p| p.options.flac_block_size = v.parse::<u32>().ok())
                    },
                }
                ChoiceField {
                    label: t("lblAdvFlacStereo"),
                    value: o.flac_stereo.map(|s| s.id().to_owned()).unwrap_or_default(),
                    options: vec![
                        (String::new(), auto.clone()),
                        ("independent".to_owned(), t("lblAdvFlacIndependent")),
                        ("leftside".to_owned(), t("lblAdvFlacLeftSide")),
                        ("rightside".to_owned(), t("lblAdvFlacRightSide")),
                        ("midside".to_owned(), t("lblAdvFlacMidSide")),
                    ],
                    on_change: {
                        let id = id.clone();
                        move |v: String| edit(&id, &|p| p.options.flac_stereo = FlacStereo::from_id(&v))
                    },
                }
                NumberField {
                    label: t("lblAdvFlacLpc"),
                    value: o.flac_max_lpc.map(u32::from),
                    min: 1,
                    max: 32,
                    auto: auto.clone(),
                    on_change: {
                        let id = id.clone();
                        move |v: Option<u32>| edit(&id, &|p| p.options.flac_max_lpc = v.map(|x| x.min(32) as u8))
                    },
                }
                NumberField {
                    label: t("lblAdvFlacPrecision"),
                    value: o.flac_lpc_precision.map(u32::from),
                    min: 8,
                    max: 15,
                    auto: auto.clone(),
                    on_change: {
                        let id = id.clone();
                        move |v: Option<u32>| edit(&id, &|p| p.options.flac_lpc_precision = v.map(|x| x.min(15) as u8))
                    },
                }
                NumberField {
                    label: t("lblAdvFlacMinPartition"),
                    value: o.flac_min_partition.map(u32::from),
                    min: 0,
                    max: 8,
                    auto: auto.clone(),
                    on_change: {
                        let id = id.clone();
                        move |v: Option<u32>| edit(&id, &|p| p.options.flac_min_partition = v.map(|x| x.min(8) as u8))
                    },
                }
                NumberField {
                    label: t("lblAdvFlacMaxPartition"),
                    value: o.flac_max_partition.map(u32::from),
                    min: 0,
                    max: 8,
                    auto: auto.clone(),
                    on_change: {
                        let id = id.clone();
                        move |v: Option<u32>| edit(&id, &|p| p.options.flac_max_partition = v.map(|x| x.min(8) as u8))
                    },
                }
                ToggleField {
                    label: t("lblAdvFlacExhaustive"),
                    checked: o.flac_exhaustive,
                    on_change: {
                        let id = id.clone();
                        move |v| edit(&id, &|p| p.options.flac_exhaustive = v)
                    },
                }
            }

            if p.is_aac() {
                NumberField {
                    label: t("lblAdvCutoff"),
                    value: o.aac_cutoff_hz,
                    min: 1000,
                    max: 24000,
                    auto: auto.clone(),
                    on_change: {
                        let id = id.clone();
                        move |v| edit(&id, &|p| p.options.aac_cutoff_hz = v)
                    },
                }
            }
            }
        }
    }
}

/// The tag-cleaning page of Settings: named entries, each a list of tags to strip and whether the
/// cover goes too. The built-in first entry keeps everything and cannot be edited.
#[component]
pub fn CleanupEditor(settings: Signal<Settings>) -> Element {
    let mut selected = use_signal(|| settings.read().cleanup_id.clone());

    let lang = settings.read().lang.clone();
    let t = |key: &str| i18n::t(&lang, key);

    let cleanups = settings.read().cleanups.clone();
    // An unknown selection falls back to the first entry; there is never none.
    let current = cleanups.iter().find(|c| c.id == selected()).or(cleanups.first()).cloned();
    let can_delete = cleanups.len() > 1;
    let cur_id = current.as_ref().map(|c| c.id.clone()).unwrap_or_default();
    // In the saved order, which the user changes by dragging.
    let listed = cleanups.clone();
    let mut dragging = use_signal(|| Option::<String>::None);
    let s_reorder = t("msgMngReorderHint");

    let s_new = t("btnMngNew");
    let s_dup = t("btnMngDuplicate");
    let s_del = t("btnMngDelete");
    let s_name = t("lblClnName");
    let s_remove_cover = t("lblClnRemoveCover");
    let s_mode = t("lblClnMode");
    let s_mode_keep = t("lblMainCleanupKeep");
    let s_mode_delete = t("lblClnModeDelete");
    let id_mode = cur_id.clone();

    let id_name = cur_id.clone();
    let id_cover = cur_id.clone();
    let id_dup = cur_id.clone();
    let id_del = cur_id.clone();

    rsx! {
        div { class: "manager",
            div { class: "manager-list",
                div { class: "manager-actions",
                    button {
                        class: "btn icon",
                        title: "{s_new}",
                        "aria-label": "{s_new}",
                        onclick: move |_| {
                            let c = TagCleanup::new(&s_new);
                            selected.set(c.id.clone());
                            let mut s = settings.write();
                            s.cleanup_id = c.id.clone();
                            s.cleanups.push(c);
                        },
                        Icon { width: 14, height: 14, icon: LdPlus }
                    }
                    button {
                        class: "btn icon",
                        title: "{s_dup}",
                        "aria-label": "{s_dup}",
                        onclick: move |_| {
                            let src = settings.read().cleanups.iter().find(|c| c.id == id_dup).cloned();
                            if let Some(mut c) = src {
                                c.id = engine::preset::new_id();
                                c.name = format!("{} 2", c.name);
                                selected.set(c.id.clone());
                                let mut s = settings.write();
                                s.cleanup_id = c.id.clone();
                                s.cleanups.push(c);
                            }
                        },
                        Icon { width: 14, height: 14, icon: LdCopy }
                    }
                    button {
                        class: "btn icon",
                        title: "{s_del}",
                        "aria-label": "{s_del}",
                        disabled: !can_delete,
                        onclick: move |_| {
                            settings.write().cleanups.retain(|c| c.id != id_del);
                            let fixed = settings.read().clone().repaired();
                            selected.set(fixed.cleanup_id.clone());
                            settings.set(fixed);
                        },
                        Icon { width: 14, height: 14, icon: LdTrash2 }
                    }
                }
                div { class: "lv-scroll",
                    for c in listed.iter() {
                        {
                            let id = c.id.clone();
                            let id_down = c.id.clone();
                            let id_over = c.id.clone();
                            let active = c.id == cur_id;
                            let moving = dragging().as_deref() == Some(c.id.as_str());
                            rsx! {
                                button {
                                    key: "{c.id}",
                                    class: match (active, moving) {
                                        (_, true) => "lv-item reordering",
                                        (true, _) => "lv-item active",
                                        _ => "lv-item",
                                    },
                                    onpointerdown: move |e| {
                                        if e.trigger_button() == Some(MouseButton::Primary) {
                                            dragging.set(Some(id_down.clone()));
                                        }
                                    },
                                    onpointerenter: move |e| {
                                        let Some(from) = dragging() else { return };
                                        if !e.held_buttons().contains(MouseButton::Primary) {
                                            dragging.set(None);
                                            return;
                                        }
                                        reorder(&mut settings.write().cleanups, |c| c.id.as_str(), &from, &id_over);
                                    },
                                    onpointerup: move |_| dragging.set(None),
                                    onclick: move |_| {
                                        selected.set(id.clone());
                                        settings.write().cleanup_id = id.clone();
                                    },
                                    div { class: "lv-item-body",
                                        div { class: "lv-item-name", "{c.name}" }
                                    }
                                }
                            }
                        }
                    }
                }
                p { class: "hint", "{s_reorder}" }
            }
            div { class: "manager-form",
                if let Some(c) = current {
                  Fragment { key: "{cur_id}",
                    div { class: "dlg-field",
                        label { class: "dlg-field-label", "{s_name}" }
                        input {
                            class: "input",
                            value: "{c.name}",
                            oninput: move |e| {
                                let v = e.value();
                                edit_cleanup(settings, &id_name, |x| x.name = v);
                            },
                        }
                    }
                    div { class: "dlg-field",
                        label { class: "dlg-field-label", "{s_mode}" }
                        select {
                            class: "select",
                            value: if c.mode == CleanupMode::Delete { "delete" } else { "keep" },
                            onchange: move |e| {
                                let mode = if e.value() == "delete" { CleanupMode::Delete } else { CleanupMode::Keep };
                                edit_cleanup(settings, &id_mode, |x| x.mode = mode);
                            },
                            option { value: "keep", selected: c.mode == CleanupMode::Keep, "{s_mode_keep}" }
                            option { value: "delete", selected: c.mode == CleanupMode::Delete, "{s_mode_delete}" }
                        }
                    }
                    ExceptionsField { settings, id: cur_id.clone(), lang: lang.clone(), mode: c.mode }
                    div { class: "dlg-field",
                        label { class: "check-label",
                            input {
                                class: "check-box",
                                r#type: "checkbox",
                                checked: c.remove_cover,
                                onchange: move |e| {
                                    let v = e.checked();
                                    edit_cleanup(settings, &id_cover, |x| x.remove_cover = v);
                                },
                            }
                            "{s_remove_cover}"
                        }
                    }
                  }
                }
            }
        }
    }
}

/// One effect of the chain. Its name is a TITLE (the dialog title look in a span, so the fold marker stays; it cannot be mistaken for
/// the label of a field), and the card folds away like the advanced options of a preset: collapsed,
/// only the titles of the chain remain. Inside: what it does, its fields, and the remove button.
/// Being on the page means it is on; removing it is turning it off.
#[component]
fn EffectCard(label: String, hint: String, remove: String, on_remove: EventHandler<()>, #[props(default)] children: Element) -> Element {
    rsx! {
        details { class: "manager-advanced", open: true,
            summary { span { class: "dlg-title", "{label}" } }
            div { class: "manager-advanced-body",
                p { class: "hint", "{hint}" }
                {children}
                div { class: "manager-actions",
                    button {
                        class: "btn icon",
                        title: "{remove}",
                        "aria-label": "{remove}",
                        onclick: move |_| on_remove.call(()),
                        Icon { width: 14, height: 14, icon: LdTrash2 }
                    }
                }
            }
        }
    }
}

/// One effect of the chain, with the fields its kind has.
#[component]
fn EffectItem(settings: Signal<Settings>, id: String, lang: String, item: Effect) -> Element {
    let t = |key: &str| i18n::t(&lang, key);
    let remove = t("tipFxRemove");
    let kind = item.kind();
    let id_rm = id.clone();
    let on_remove = move |_| edit_effects(settings, &id_rm, |x| x.remove(kind));
    let (i1, i2, i3, i4) = (id.clone(), id.clone(), id.clone(), id.clone());
    let i5 = id.clone();
    match item {
        Effect::TrimSilence(tr) => rsx! {
            EffectCard { label: t("lblFxTrimSilence"), hint: t("msgFxTrimSilenceHint"), remove, on_remove,
                ToggleField {
                    label: t("lblFxTrimStart"),
                    checked: tr.start,
                    on_change: move |v| edit_trim(settings, &i1, |x| x.start = v),
                }
                ToggleField {
                    label: t("lblFxTrimEnd"),
                    checked: tr.end,
                    on_change: move |v| edit_trim(settings, &i2, |x| x.end = v),
                }
                NumberField {
                    label: t("lblFxSilenceThreshold"),
                    value: Some((-tr.threshold_db).round() as u32),
                    min: 10,
                    max: 120,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_trim(settings, &i3, |x| x.threshold_db = -f64::from(v.unwrap_or(60))),
                }
                NumberField {
                    label: t("lblFxTrimMargin"),
                    value: Some(tr.margin_ms),
                    min: 0,
                    max: 5000,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_trim(settings, &i4, |x| x.margin_ms = v.unwrap_or(50)),
                }
            }
        },
        Effect::CutRange(c) => rsx! {
            EffectCard { label: t("lblFxCutRange"), hint: t("msgFxCutRangeHint"), remove, on_remove,
                NumberField {
                    label: t("lblFxFrom"),
                    value: Some(c.from_s.round() as u32),
                    min: 0,
                    max: 86_400,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_cut(settings, &i1, |x| x.from_s = f64::from(v.unwrap_or(0))),
                }
                NumberField {
                    label: t("lblFxTo"),
                    value: Some(c.to_s.round() as u32),
                    min: 0,
                    max: 86_400,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_cut(settings, &i2, |x| x.to_s = f64::from(v.unwrap_or(0))),
                }
            }
        },
        Effect::LimitDuration(l) => rsx! {
            EffectCard { label: t("lblFxLimit"), hint: t("msgFxLimitHint"), remove, on_remove,
                NumberField {
                    label: t("lblFxLimitSeconds"),
                    value: Some(l.seconds.round() as u32),
                    min: 1,
                    max: 86_400,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_limit(settings, &i1, |x| x.seconds = f64::from(v.unwrap_or(30))),
                }
            }
        },
        Effect::Channels(c) => {
            let modes = vec![
                ("swap".to_owned(), t("lblFxChSwap")),
                ("left".to_owned(), t("lblFxChLeft")),
                ("right".to_owned(), t("lblFxChRight")),
                ("mono".to_owned(), t("lblFxChMono")),
            ];
            let current = match c.mode {
                ChannelsMode::Swap => "swap",
                ChannelsMode::Left => "left",
                ChannelsMode::Right => "right",
                ChannelsMode::Mono => "mono",
            };
            rsx! {
                EffectCard { label: t("lblFxChannels"), hint: t("msgFxChannelsHint"), remove, on_remove,
                    ChoiceField {
                        label: t("lblFxMode"),
                        value: current.to_owned(),
                        options: modes,
                        on_change: move |m: String| {
                            let m = match m.as_str() {
                                "left" => ChannelsMode::Left,
                                "right" => ChannelsMode::Right,
                                "mono" => ChannelsMode::Mono,
                                _ => ChannelsMode::Swap,
                            };
                            edit_channels(settings, &i1, |x| x.mode = m);
                        },
                    }
                }
            }
        }
        Effect::Repair(r) => rsx! {
            EffectCard { label: t("lblFxRepair"), hint: t("msgFxRepairHint"), remove, on_remove,
                ToggleField {
                    label: t("lblFxDeclick"),
                    checked: r.declick,
                    on_change: move |v| edit_repair(settings, &i1, |x| x.declick = v),
                }
                ToggleField {
                    label: t("lblFxDeclip"),
                    checked: r.declip,
                    on_change: move |v| edit_repair(settings, &i2, |x| x.declip = v),
                }
            }
        },
        Effect::LowCut(l) => rsx! {
            EffectCard { label: t("lblFxLowCut"), hint: t("msgFxLowCutHint"), remove, on_remove,
                NumberField {
                    label: t("lblFxLowCutHz"),
                    value: Some(l.hz),
                    min: 10,
                    max: 1000,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_low_cut(settings, &i1, |x| x.hz = v.unwrap_or(30)),
                }
            }
        },
        Effect::Denoise(d) => rsx! {
            EffectCard { label: t("lblFxDenoise"), hint: t("msgFxDenoiseHint"), remove, on_remove,
                NumberField {
                    label: t("lblFxDenoiseDb"),
                    value: Some(d.reduction_db),
                    min: 1,
                    max: 60,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_denoise(settings, &i1, |x| x.reduction_db = v.unwrap_or(12)),
                }
                NumberField {
                    label: t("lblFxDenoiseFloor"),
                    value: Some((-d.floor_db).round() as u32),
                    min: 20,
                    max: 80,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_denoise(settings, &i2, |x| x.floor_db = -f64::from(v.unwrap_or(50))),
                }
            }
        },
        Effect::DeEsser(d) => rsx! {
            EffectCard { label: t("lblFxDeEsser"), hint: t("msgFxDeEsserHint"), remove, on_remove,
                NumberField {
                    label: t("lblFxDeEsserPct"),
                    value: Some(d.intensity_pct),
                    min: 0,
                    max: 100,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_de_esser(settings, &i1, |x| x.intensity_pct = v.unwrap_or(50)),
                }
            }
        },
        Effect::Equalizer(q) => rsx! {
            EffectCard { label: t("lblFxEqualizer"), hint: t("msgFxEqualizerHint"), remove, on_remove,
                for (band, hz) in EQ_BANDS_HZ.iter().copied().enumerate() {
                    SignedField {
                        key: "{hz}",
                        label: format!("{hz} Hz"),
                        value: q.gains_db[band],
                        min: -12,
                        max: 12,
                        on_change: {
                            let id = id.clone();
                            move |v: i32| edit_equalizer(settings, &id, |x| x.gains_db[band] = v)
                        },
                    }
                }
            }
        },
        Effect::Tone(tn) => rsx! {
            EffectCard { label: t("lblFxTone"), hint: t("msgFxToneHint"), remove, on_remove,
                SignedField {
                    label: t("lblFxBass"),
                    value: tn.bass_db,
                    min: -12,
                    max: 12,
                    on_change: move |v: i32| edit_tone(settings, &i1, |x| x.bass_db = v),
                }
                SignedField {
                    label: t("lblFxTreble"),
                    value: tn.treble_db,
                    min: -12,
                    max: 12,
                    on_change: move |v: i32| edit_tone(settings, &i2, |x| x.treble_db = v),
                }
            }
        },
        Effect::Dynamics(d) => {
            let modes = vec![
                ("compressor".to_owned(), t("lblFxDynCompressor")),
                ("normaliser".to_owned(), t("lblFxDynNormaliser")),
            ];
            let current = if d.mode == DynamicsMode::Compressor { "compressor" } else { "normaliser" };
            rsx! {
                EffectCard { label: t("lblFxDynamics"), hint: t("msgFxDynamicsHint"), remove, on_remove,
                    ChoiceField {
                        label: t("lblFxMode"),
                        value: current.to_owned(),
                        options: modes,
                        on_change: move |m: String| {
                            let m = if m == "normaliser" { DynamicsMode::Normaliser } else { DynamicsMode::Compressor };
                            edit_dynamics(settings, &i1, |x| x.mode = m);
                        },
                    }
                    if d.mode == DynamicsMode::Compressor {
                        NumberField {
                            label: t("lblFxDynThreshold"),
                            value: Some((-d.threshold_db).round() as u32),
                            min: 1,
                            max: 60,
                            auto: String::new(),
                            on_change: move |v: Option<u32>| edit_dynamics(settings, &i2, |x| x.threshold_db = -f64::from(v.unwrap_or(20))),
                        }
                        NumberField {
                            label: t("lblFxDynRatio"),
                            value: Some(d.ratio),
                            min: 1,
                            max: 20,
                            auto: String::new(),
                            on_change: move |v: Option<u32>| edit_dynamics(settings, &i3, |x| x.ratio = v.unwrap_or(3)),
                        }
                    }
                }
            }
        }
        Effect::Tempo(tp) => rsx! {
            EffectCard { label: t("lblFxTempo"), hint: t("msgFxTempoHint"), remove, on_remove,
                SignedField {
                    label: t("lblFxTempoPct"),
                    value: tp.percent,
                    min: -50,
                    max: 100,
                    on_change: move |v: i32| edit_tempo(settings, &i1, |x| x.percent = v),
                }
            }
        },
        Effect::Volume(v) => {
            let mode = match v.mode {
                VolumeMode::Loudness => "loudness",
                VolumeMode::Peak => "peak",
                VolumeMode::ReplayGain => "replay_gain",
                VolumeMode::Gain => "gain",
            };
            let modes = vec![
                ("loudness".to_owned(), t("lblFxModeLoudness")),
                ("peak".to_owned(), t("lblFxModePeak")),
                ("replay_gain".to_owned(), t("lblFxModeReplayGain")),
                ("gain".to_owned(), t("lblFxModeGain")),
            ];
            let (ip, id_dir, ia) = (id.clone(), id.clone(), id.clone());
            let louder = v.gain_db >= 0.0;
            let direction = vec![("louder".to_owned(), t("lblFxGainLouder")), ("quieter".to_owned(), t("lblFxGainQuieter"))];
            rsx! {
                EffectCard { label: t("lblFxVolume"), hint: t("msgFxVolumeHint"), remove, on_remove,
                    ChoiceField {
                        label: t("lblFxMode"),
                        value: mode.to_owned(),
                        options: modes,
                        on_change: move |m: String| {
                            let m = match m.as_str() {
                                "peak" => VolumeMode::Peak,
                                "replay_gain" => VolumeMode::ReplayGain,
                                "gain" => VolumeMode::Gain,
                                _ => VolumeMode::Loudness,
                            };
                            edit_volume(settings, &i1, |x| x.mode = m);
                        },
                    }
                    if v.mode == VolumeMode::Loudness {
                        NumberField {
                            label: t("lblFxTarget"),
                            value: Some((-v.target_lufs).round() as u32),
                            min: 5,
                            max: 50,
                            auto: String::new(),
                            on_change: move |n: Option<u32>| edit_volume(settings, &i2, |x| x.target_lufs = -f64::from(n.unwrap_or(16))),
                        }
                    }
                    if v.mode == VolumeMode::Peak {
                        NumberField {
                            label: t("lblFxTargetPeak"),
                            value: Some((-v.target_peak_db).round() as u32),
                            min: 0,
                            max: 30,
                            auto: String::new(),
                            on_change: move |n: Option<u32>| edit_volume(settings, &ip, |x| x.target_peak_db = -f64::from(n.unwrap_or(1))),
                        }
                    }
                    if v.mode == VolumeMode::Gain {
                        ChoiceField {
                            label: t("lblFxGain"),
                            value: if louder { "louder".to_owned() } else { "quieter".to_owned() },
                            options: direction,
                            on_change: move |d: String| {
                                edit_volume(settings, &id_dir, |x| {
                                    let m = x.gain_db.abs();
                                    x.gain_db = if d == "quieter" { -m } else { m };
                                });
                            },
                        }
                        NumberField {
                            label: t("lblFxGainDb"),
                            value: Some(v.gain_db.abs().round() as u32),
                            min: 0,
                            max: 30,
                            auto: String::new(),
                            on_change: move |n: Option<u32>| {
                                let m = f64::from(n.unwrap_or(0));
                                edit_volume(settings, &i3, |x| x.gain_db = if x.gain_db < 0.0 { -m } else { m });
                            },
                        }
                    }
                    if v.mode == VolumeMode::ReplayGain {
                        ToggleField {
                            label: t("lblFxUseAlbum"),
                            checked: v.album,
                            on_change: move |a| edit_volume(settings, &ia, |x| x.album = a),
                        }
                    }
                    if v.mode != VolumeMode::Peak {
                        NumberField {
                            label: t("lblFxCeiling"),
                            value: Some((-v.ceiling_db).round() as u32),
                            min: 0,
                            max: 20,
                            auto: String::new(),
                            on_change: move |n: Option<u32>| edit_volume(settings, &i4, |x| x.ceiling_db = -f64::from(n.unwrap_or(1))),
                        }
                    }
                }
            }
        }
        Effect::FadeIn(fd) => rsx! {
            EffectCard { label: t("lblFxFadeIn"), hint: t("msgFxFadeInHint"), remove, on_remove,
                NumberField {
                    label: t("lblFxFadeMs"),
                    value: Some(fd.ms),
                    min: 0,
                    max: 60_000,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_fade(settings, &i1, Kind::FadeIn, |x| x.ms = v.unwrap_or(2000)),
                }
            }
        },
        Effect::FadeOut(fd) => rsx! {
            EffectCard { label: t("lblFxFadeOut"), hint: t("msgFxFadeOutHint"), remove, on_remove,
                NumberField {
                    label: t("lblFxFadeMs"),
                    value: Some(fd.ms),
                    min: 0,
                    max: 60_000,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_fade(settings, &i1, Kind::FadeOut, |x| x.ms = v.unwrap_or(2000)),
                }
            }
        },
        Effect::SilenceEnd(se) => rsx! {
            EffectCard { label: t("lblFxSilenceEnd"), hint: t("msgFxSilenceEndHint"), remove, on_remove,
                NumberField {
                    label: t("lblFxSilenceSeconds"),
                    value: Some(se.seconds.round() as u32),
                    min: 0,
                    max: 60,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_silence(settings, &i1, |x| x.seconds = f64::from(v.unwrap_or(2))),
                }
                NumberField {
                    label: t("lblFxSilenceThreshold"),
                    value: Some((-se.threshold_db).round() as u32),
                    min: 10,
                    max: 120,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_silence(settings, &i2, |x| x.threshold_db = -f64::from(v.unwrap_or(60))),
                }
                NumberField {
                    label: t("lblFxSilenceMargin"),
                    value: Some(se.margin_ms),
                    min: 0,
                    max: 5000,
                    auto: String::new(),
                    on_change: move |v: Option<u32>| edit_silence(settings, &i3, |x| x.margin_ms = v.unwrap_or(50)),
                }
            }
        },
        Effect::ReplayGain(r) => rsx! {
            EffectCard { label: t("lblFxReplayGain"), hint: t("msgFxReplayGainHint"), remove, on_remove,
                ToggleField {
                    label: t("lblFxReplayGainAlbum"),
                    checked: r.album,
                    on_change: move |a| edit_replaygain(settings, &i5, |x| x.album = a),
                }
            }
        },
    }
}

/// The Effects tab of Settings: the list of named chains on the left; on the right the effects of the
/// selected chain - a list, in the fixed order they run in - and a menu to add another kind.
///
/// The loudness and level fields are whole numbers shown as a distance under full scale (16 =
/// -16 LUFS), because the catalog's number field is a whole-number control and a level is never above it.
#[component]
pub fn EffectsEditor(settings: Signal<Settings>) -> Element {
    let mut selected = use_signal(|| settings.read().effects_id.clone());

    let lang = settings.read().lang.clone();
    let t = |key: &str| i18n::t(&lang, key);

    let entries = settings.read().effects.clone();
    let current = entries.iter().find(|e| e.id == selected()).or(entries.first()).cloned();
    let can_delete = entries.len() > 1;
    let cur_id = current.as_ref().map(|e| e.id.clone()).unwrap_or_default();
    let mut dragging = use_signal(|| Option::<String>::None);
    let s_reorder = t("msgMngReorderHint");

    let s_new = t("btnMngNew");
    let s_dup = t("btnMngDuplicate");
    let s_del = t("btnMngDelete");
    let id_dup = cur_id.clone();
    let id_del = cur_id.clone();

    rsx! {
        div { class: "manager",
            div { class: "manager-list",
                div { class: "manager-actions",
                    button {
                        class: "btn icon",
                        title: "{s_new}",
                        "aria-label": "{s_new}",
                        onclick: move |_| {
                            let e = EffectsEntry::new(&s_new);
                            selected.set(e.id.clone());
                            let mut s = settings.write();
                            s.effects_id = e.id.clone();
                            s.effects.push(e);
                        },
                        Icon { width: 14, height: 14, icon: LdPlus }
                    }
                    button {
                        class: "btn icon",
                        title: "{s_dup}",
                        "aria-label": "{s_dup}",
                        onclick: move |_| {
                            let src = settings.read().effects.iter().find(|e| e.id == id_dup).cloned();
                            if let Some(mut e) = src {
                                e.id = engine::preset::new_id();
                                e.name = format!("{} 2", e.name);
                                selected.set(e.id.clone());
                                let mut s = settings.write();
                                s.effects_id = e.id.clone();
                                s.effects.push(e);
                            }
                        },
                        Icon { width: 14, height: 14, icon: LdCopy }
                    }
                    button {
                        class: "btn icon",
                        title: "{s_del}",
                        "aria-label": "{s_del}",
                        disabled: !can_delete,
                        onclick: move |_| {
                            settings.write().effects.retain(|e| e.id != id_del);
                            let fixed = settings.read().clone().repaired();
                            selected.set(fixed.effects_id.clone());
                            settings.set(fixed);
                        },
                        Icon { width: 14, height: 14, icon: LdTrash2 }
                    }
                }
                div { class: "lv-scroll",
                    for e in entries.iter() {
                        {
                            let id = e.id.clone();
                            let id_down = e.id.clone();
                            let id_over = e.id.clone();
                            let active = e.id == cur_id;
                            let moving = dragging().as_deref() == Some(e.id.as_str());
                            rsx! {
                                button {
                                    key: "{e.id}",
                                    class: match (active, moving) {
                                        (_, true) => "lv-item reordering",
                                        (true, _) => "lv-item active",
                                        _ => "lv-item",
                                    },
                                    onpointerdown: move |ev| {
                                        if ev.trigger_button() == Some(MouseButton::Primary) {
                                            dragging.set(Some(id_down.clone()));
                                        }
                                    },
                                    onpointerenter: move |ev| {
                                        let Some(from) = dragging() else { return };
                                        if !ev.held_buttons().contains(MouseButton::Primary) {
                                            dragging.set(None);
                                            return;
                                        }
                                        reorder(&mut settings.write().effects, |e| e.id.as_str(), &from, &id_over);
                                    },
                                    onpointerup: move |_| dragging.set(None),
                                    onclick: move |_| {
                                        selected.set(id.clone());
                                        settings.write().effects_id = id.clone();
                                    },
                                    div { class: "lv-item-body",
                                        div { class: "lv-item-name", "{e.name}" }
                                    }
                                }
                            }
                        }
                    }
                }
                p { class: "hint", "{s_reorder}" }
            }
            div { class: "manager-form",
                if let Some(e) = current {
                  Fragment { key: "{cur_id}",
                    {
                        let id = cur_id.clone();
                        let i_name = id.clone();
                        let i_add = id.clone();
                        let addable = e.addable();
                        let s_add = t("lblFxAdd");
                        rsx! {
                            div { class: "dlg-field",
                                label { class: "dlg-field-label", {t("lblFxName")} }
                                input {
                                    class: "input",
                                    value: "{e.name}",
                                    oninput: move |ev| {
                                        let v = ev.value();
                                        edit_effects(settings, &i_name, |x| x.name = v);
                                    },
                                }
                            }
                            for item in e.items.clone() {
                                EffectItem { settings, id: id.clone(), lang: lang.clone(), item }
                            }
                            if !addable.is_empty() {
                                div { class: "dlg-field",
                                    select {
                                        class: "select",
                                        title: "{s_add}",
                                        "aria-label": "{s_add}",
                                        value: "",
                                        onchange: move |ev| {
                                            if let Some(k) = kind_from_value(&ev.value()) {
                                                edit_effects(settings, &i_add, |x| x.add(k));
                                            }
                                        },
                                        option { value: "", selected: true, "{s_add}" }
                                        for k in addable {
                                            option {
                                                key: "{kind_value(k)}",
                                                value: "{kind_value(k)}",
                                                {kind_label(k, &lang)}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                  }
                }
            }
        }
    }
}

/// The Notifications tab of Settings.
#[component]
pub fn NotifyEditor(settings: Signal<Settings>) -> Element {
    let lang = settings.read().lang.clone();
    let t = |key: &str| i18n::t(&lang, key);
    let s_notify = t("lblStgNotify");
    let on = settings.read().notify;
    rsx! {
        div { class: "manager",
            div { class: "manager-form",
                div { class: "dlg-field",
                    label { class: "check-label",
                        input {
                            class: "check-box",
                            r#type: "checkbox",
                            checked: on,
                            onchange: move |e| settings.write().notify = e.checked(),
                        }
                        "{s_notify}"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{entry_span, reorder, replace_span};

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_caret_picks_its_entry_and_a_suggestion_replaces_only_that_entry() {
        let text = "COMMENT\nAR;GENRE";
        let name = |pos| {
            let (s, e) = entry_span(text, pos);
            &text[s..e]
        };
        assert_eq!(name(0), "COMMENT");
        assert_eq!(name(9), "AR"); // between the A and the R
        assert_eq!(name(10), "AR"); // right before the semicolon
        assert_eq!(name(11), "GENRE"); // right after it
        assert_eq!(name(usize::MAX), "GENRE");
        let (out, caret) = replace_span(text, entry_span(text, 9), "ARTISTS");
        assert_eq!(out, "COMMENT\nARTISTS;GENRE");
        assert_eq!(caret, "COMMENT\nARTISTS".len());
    }

    #[test]
    fn dragging_takes_the_slot_of_the_entry_it_is_dropped_on() {
        let mut v = ids(&["a", "b", "c"]);
        reorder(&mut v, |s| s.as_str(), "a", "c");
        assert_eq!(v, ids(&["b", "c", "a"]));
        reorder(&mut v, |s| s.as_str(), "a", "b");
        assert_eq!(v, ids(&["a", "b", "c"]));
    }

    #[test]
    fn an_unknown_or_identical_entry_changes_nothing() {
        let mut v = ids(&["a", "b"]);
        reorder(&mut v, |s| s.as_str(), "a", "a");
        reorder(&mut v, |s| s.as_str(), "x", "b");
        reorder(&mut v, |s| s.as_str(), "a", "x");
        assert_eq!(v, ids(&["a", "b"]));
    }
}
