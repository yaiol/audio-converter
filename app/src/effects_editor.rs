//! The effects editor, a page of the Settings view: the entries on the left; on the right the name of the selected one and a card for
//! each effect it holds, in the order they run (drag to move, fold, remove), with a menu that adds one. The same file in the Video
//! Converter and the Audio Converter, so the two cannot differ: what an app is (`effects::WORKS`, "video" or "audio") is the only thing
//! it says.
//!
//! ⚠ SOURCE OF TRUTH: the shared ffmpeg-effects folder (`rust/app`). A copy in an app is never edited there.
//! It needs, from the app: `crate::settings::Settings` (`effects`, `effects_id`, `lang`, `repaired()`), `crate::i18n`, `crate::manager::reorder`,
//! `crate::effects_form`, the catalog's `ComboItem` / `Combobox` / `Popover`, and `engine::effects` (`catalog`, `Item`, `EffectsEntry`, `WORKS`).

use crate::{
    catalog::{ComboItem, Combobox, Popover},
    effects_form::{BrowseRequest, CatalogRef, EffectForm, FormText, Named},
    i18n,
    manager::reorder,
    settings::Settings,
};
use dioxus::{html::input_data::MouseButton, prelude::*};
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdChevronDown, LdChevronRight, LdChevronsDownUp, LdChevronsUpDown, LdCopy, LdGripVertical, LdListFilter, LdPlus, LdTrash2},
};
use engine::{
    effects::{self, EffectsEntry, Item},
    ffmpeg_effects::{V, Values},
};

/// Apply `edit` to the effects entry with this id and keep it inside what an entry can be.
fn edit_effects(mut settings: Signal<Settings>, id: &str, edit: impl FnOnce(&mut EffectsEntry)) {
    let mut s = settings.write();
    if let Some(e) = s.effects.iter_mut().find(|e| e.id == id) {
        edit(e);
        *e = e.clone().normalized();
    }
}

/// One effect of the entry as a card. The header is always there, collapsed or not: a grip to drag the card to another place
/// in the running order, the title (a click opens or closes the form) and the remove button. Being on the page means it is on;
/// removing it is turning it off.
#[component]
fn EffectCard(
    label: String,
    remove: String,
    drag: String,
    expanded: bool,
    moving: bool,
    on_toggle: EventHandler<()>,
    on_remove: EventHandler<()>,
    on_drag_start: EventHandler<()>,
    on_drag_over: EventHandler<()>,
    #[props(default)] children: Element,
) -> Element {
    rsx! {
        div { class: if moving { "fx-card moving" } else { "fx-card" },
            div {
                class: "fx-head",
                onpointerenter: move |e| {
                    if e.held_buttons().contains(MouseButton::Primary) {
                        on_drag_over.call(());
                    }
                },
                span {
                    class: "fx-grip",
                    title: "{drag}",
                    "aria-label": "{drag}",
                    onpointerdown: move |e| {
                        if e.trigger_button() == Some(MouseButton::Primary) {
                            on_drag_start.call(());
                        }
                    },
                    Icon { width: 14, height: 14, icon: LdGripVertical }
                }
                button {
                    class: "fx-title",
                    r#type: "button",
                    "aria-expanded": "{expanded}",
                    onclick: move |_| on_toggle.call(()),
                    if expanded {
                        Icon { width: 14, height: 14, icon: LdChevronDown }
                    } else {
                        Icon { width: 14, height: 14, icon: LdChevronRight }
                    }
                    span { class: "dlg-title", "{label}" }
                }
                button {
                    class: "btn icon",
                    r#type: "button",
                    title: "{remove}",
                    "aria-label": "{remove}",
                    onclick: move |_| on_remove.call(()),
                    Icon { width: 14, height: 14, icon: LdTrash2 }
                }
            }
            if expanded {
                div { class: "fx-body", {children} }
            }
        }
    }
}

/// Every word the effect forms write, in the user's language.
fn form_text(lang: &str) -> FormText {
    let t = |key: &str| i18n::t(lang, key);
    FormText {
        nothing: t("msgFxNothingToSet"),
        browse: t("btnDstBrowse"),
        named_size: t("lblFxNamedSize"),
        pick_colour: t("tipFxPickColour"),
        cancel: t("btnFxCancel"),
        apply: t("btnMainApply"),
        not_a_rate: t("msgFxNotARate"),
        not_a_time: t("msgFxNotATime"),
        placeholder_rate: t("phFxRate"),
        placeholder_time: t("phFxTime"),
        placeholder_colour: t("phFxColour"),
        placeholder_file: t("phFxFile"),
        placeholder_folder: t("phFxFolder"),
        placeholder_save: t("phFxSave"),
        reset: t("tipFxReset"),
    }
}

/// The names ffmpeg knows for sizes, frame rates and formats, which the forms offer.
fn named() -> Named {
    static NAMED: std::sync::OnceLock<Named> = std::sync::OnceLock::new();
    NAMED.get_or_init(|| Named::from_json(include_str!("../../engine/data/ffmpeg-named-values.json")).expect("the shipped names read")).clone()
}

/// The system the catalogue is asked about (what ffmpeg builds exist for).
const SYSTEM: &str = if cfg!(windows) {
    "windows"
} else if cfg!(target_os = "macos") {
    "macos"
} else {
    "linux"
};

/// The "Add an effect" menu: the effects the catalogue offers for what the app converts, under the headings of its categories, each group
/// alphabetical by title.
fn menu() -> Vec<(String, String, Vec<(String, String, String)>)> {
    let Some(cat) = effects::catalog() else { return Vec::new() };
    let mut entries = cat.picker(SYSTEM, effects::WORKS);
    entries.sort_by_key(|e| e.title.to_lowercase());
    cat.categories
        .iter()
        .filter_map(|c| {
            let items: Vec<(String, String, String)> = entries
                .iter()
                .filter(|e| e.category == c.id)
                .map(|e| (e.key.clone(), e.title.clone(), cat.form(&e.key).and_then(|f| f.what).unwrap_or_default()))
                .collect();
            (!items.is_empty()).then(|| (c.id.clone(), c.title.clone(), items))
        })
        .collect()
}

/// The rows of the "Add an effect" list for what was typed, in the chosen category (empty = all of them, each under a heading):
/// the effects whose title, description or category contains the text, case ignored. Each row is (key, text, is a heading).
fn menu_rows(query: &str, category: &str) -> Vec<(String, String, bool)> {
    let q = query.trim().to_lowercase();
    let mut rows = Vec::new();
    for (id, heading, items) in menu() {
        if !category.is_empty() && id != category {
            continue;
        }
        let all = q.is_empty() || heading.to_lowercase().contains(&q);
        let hits: Vec<_> = items.into_iter().filter(|(_, title, what)| all || title.to_lowercase().contains(&q) || what.to_lowercase().contains(&q)).collect();
        if hits.is_empty() {
            continue;
        }
        if category.is_empty() {
            rows.push((format!("heading:{heading}"), heading, true));
        }
        rows.extend(hits.into_iter().map(|(k, t, _)| (k, t, false)));
    }
    rows
}

/// One effect of the entry: its card and its form, written back to the entry at once.
#[component]
fn EffectItem(settings: Signal<Settings>, id: String, lang: String, at: usize, item: Item, open: Signal<Vec<bool>>, drag: Signal<Option<usize>>) -> Element {
    let t = |key: &str| i18n::t(&lang, key);
    let (id_remove, id_over, id_set, id_browse) = (id.clone(), id.clone(), id.clone(), id.clone());
    let mut open = open;
    let mut drag = drag;
    let key = item.key.clone();
    let browse_item = item.clone();

    rsx! {
        EffectCard {
            label: item.title(),
            remove: t("tipFxRemove"),
            drag: t("tipFxDrag"),
            expanded: open.read().get(at).copied().unwrap_or(true),
            moving: drag() == Some(at),
            on_toggle: move |_| {
                let mut v = open.write();
                if v.len() <= at {
                    v.resize(at + 1, true);
                }
                v[at] = !v[at];
            },
            on_remove: move |_| {
                edit_effects(settings, &id_remove, |e| {
                    if at < e.items.len() {
                        e.items.remove(at);
                    }
                });
                let mut v = open.write();
                if at < v.len() {
                    v.remove(at);
                }
            },
            on_drag_start: move |_| drag.set(Some(at)),
            on_drag_over: move |_| {
                let Some(from) = drag() else { return };
                if from == at {
                    return;
                }
                edit_effects(settings, &id_over, |e| {
                    if from < e.items.len() && at < e.items.len() {
                        let moved = e.items.remove(from);
                        e.items.insert(at, moved);
                    }
                });
                let mut v = open.write();
                let top = from.max(at) + 1;
                if v.len() < top {
                    v.resize(top, true);
                }
                let moved = v.remove(from);
                v.insert(at, moved);
                drag.set(Some(at));
            },
            if let Some(cat) = effects::catalog() {
              EffectForm {
                catalog: CatalogRef(cat),
                effect: key,
                values: item.all_values(),
                text: form_text(&lang),
                named: named(),
                on_change: move |v: Values| edit_effects(settings, &id_set, |e| {
                    if let Some(x) = e.items.get_mut(at) {
                        x.set_values(&v);
                    }
                }),
                on_browse: move |req: BrowseRequest| {
                    let (id, item) = (id_browse.clone(), browse_item.clone());
                    spawn(async move {
                        let picked = match req.kind.as_str() {
                            "folder" => rfd::AsyncFileDialog::new().pick_folder().await,
                            "saveFile" => rfd::AsyncFileDialog::new().save_file().await,
                            _ => rfd::AsyncFileDialog::new().pick_file().await,
                        };
                        if let Some(f) = picked {
                            let mut values = item.all_values();
                            values.insert(req.id.clone(), V::Str(f.path().to_string_lossy().into_owned()));
                            edit_effects(settings, &id, |e| {
                                if let Some(x) = e.items.get_mut(at) {
                                    x.set_values(&values);
                                }
                            });
                        }
                    });
                },
              }
            }
        }
    }
}

/// The effects editor, a page of the Settings view: the entries on the left; on the right the name of the
/// selected one and a card for each effect it holds, in the order they run, with a menu that adds one.
#[component]
pub fn EffectsEditor(settings: Signal<Settings>) -> Element {
    let mut selected = use_signal(|| settings.read().effects_id.clone());
    let mut dragging = use_signal(|| Option::<String>::None);
    let mut query = use_signal(String::new);
    let mut category = use_signal(String::new);
    let mut category_open = use_signal(|| false);
    // which cards are open, by position (a card past the end counts as open), and the card being dragged
    let mut open = use_signal(Vec::<bool>::new);
    let mut drag = use_signal(|| Option::<usize>::None);

    let lang = settings.read().lang.clone();
    let t = |key: &str| i18n::t(&lang, key);

    let entries = settings.read().effects.clone();
    let current = entries.iter().find(|e| e.id == selected()).or(entries.first()).cloned();
    let cur_id = current.as_ref().map(|e| e.id.clone()).unwrap_or_default();

    let s_new = t("btnMngNew");
    let s_dup = t("btnMngDuplicate");
    let s_del = t("btnMngDelete");
    let s_reorder = t("msgMngReorderHint");
    let s_add = t("lblFxAdd");
    let s_all = t("optFxCategoryAll");

    let id_for_dup = cur_id.clone();
    let id_for_del = cur_id.clone();
    let id_for_name = cur_id.clone();
    let id_for_add = cur_id.clone();

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
                            settings.write().effects.push(e);
                        },
                        Icon { width: 14, height: 14, icon: LdPlus }
                    }
                    button {
                        class: "btn icon",
                        title: "{s_dup}",
                        "aria-label": "{s_dup}",
                        disabled: current.is_none(),
                        onclick: move |_| {
                            let src = settings.read().effects.iter().find(|e| e.id == id_for_dup).cloned();
                            if let Some(mut e) = src {
                                e.id = engine::preset::new_id();
                                e.name = format!("{} 2", e.name);
                                selected.set(e.id.clone());
                                settings.write().effects.push(e);
                            }
                        },
                        Icon { width: 14, height: 14, icon: LdCopy }
                    }
                    button {
                        class: "btn icon",
                        title: "{s_del}",
                        "aria-label": "{s_del}",
                        disabled: current.is_none(),
                        onclick: move |_| {
                            settings.write().effects.retain(|e| e.id != id_for_del);
                            let fixed = settings.read().clone().repaired();
                            selected.set(fixed.effects.first().map(|e| e.id.clone()).unwrap_or_default());
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
                                        open.set(Vec::new());
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
            // The key makes the fields remount when another entry is selected.
            div {
                class: if drag().is_some() { "manager-form fx-dragging" } else { "manager-form" },
                key: "{cur_id}",
                onpointerup: move |_| drag.set(None),
                onpointerleave: move |_| drag.set(None),
                if let Some(cur) = &current {
                    div { class: "dlg-field",
                        label { class: "dlg-field-label", {t("lblPrsName")} }
                        div { class: "fx-name-row",
                            input {
                                class: "input",
                                value: "{cur.name}",
                                oninput: move |ev| {
                                    let v = ev.value();
                                    edit_effects(settings, &id_for_name, |x| x.name = v);
                                },
                            }
                            {
                                // one button: close every card, or open every card when all are closed already
                                let count = cur.items.len();
                                let any_open = (0..count).any(|i| open.read().get(i).copied().unwrap_or(true));
                                let tip = if any_open { t("tipFxCollapseAll") } else { t("tipFxExpandAll") };
                                rsx! {
                                    button {
                                        class: "btn icon",
                                        r#type: "button",
                                        title: "{tip}",
                                        "aria-label": "{tip}",
                                        disabled: count == 0,
                                        onclick: move |_| open.set(vec![!any_open; count]),
                                        if any_open {
                                            Icon { width: 14, height: 14, icon: LdChevronsDownUp }
                                        } else {
                                            Icon { width: 14, height: 14, icon: LdChevronsUpDown }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    for (at, item) in cur.items.clone().into_iter().enumerate() {
                        EffectItem { key: "{at}-{item.key}", settings, id: cur_id.clone(), lang: lang.clone(), at, item, open, drag }
                    }
                    if effects::catalog().is_none() {
                        p { class: "hint", {t("msgFxNoCatalogue")} }
                    }
                    for problem in cur.order_problems() {
                        p { class: "hint", key: "{problem}", "{problem}" }
                    }
                    div { class: "dlg-field",
                        div { class: "barh-grp fx-add",
                            {
                                let menu = menu();
                                let current_title = menu.iter().find(|(id, _, _)| *id == category()).map(|(_, title, _)| title.clone()).unwrap_or_else(|| s_all.clone());
                                rsx! {
                                    button {
                                        id: "fx-category",
                                        r#type: "button",
                                        class: "btn icon",
                                        title: "{current_title}",
                                        "aria-label": "{current_title}",
                                        onclick: move |_| category_open.set(!category_open()),
                                        Icon { width: 14, height: 14, icon: LdListFilter }
                                    }
                                }
                            }
                            div { class: "fx-add-combo",
                                {
                                    let rows = menu_rows(&query(), &category());
                                    let items: Vec<ComboItem> = rows.iter().map(|(k, l, header)| ComboItem { key: k.clone(), label: l.clone(), active: false, header: *header }).collect();
                                    rsx! {
                                        Combobox {
                                            value: query(),
                                            placeholder: s_add.clone(),
                                            items,
                                            on_change: move |v: String| query.set(v),
                                            on_pick: move |i: usize| {
                                                if let Some((key, _, false)) = menu_rows(&query(), &category()).get(i) {
                                                    edit_effects(settings, &id_for_add, |x| x.items.push(Item::new(key)));
                                                    query.set(String::new());
                                                }
                                            },
                                        }
                                    }
                                }
                            }
                        }
                        // outside the weld: a `.barh-grp > *` rule gives every child the bar's height and no border, which would stretch the panel
                        {
                            let menu = menu();
                            rsx! {
                                Popover { anchor_id: "fx-category".to_owned(), open: category_open(), on_close: move |_| category_open.set(false),
                                    button {
                                        r#type: "button",
                                        class: if category().is_empty() { "pop-item active" } else { "pop-item" },
                                        onmousedown: move |ev| {
                                            ev.prevent_default();
                                            category.set(String::new());
                                            category_open.set(false);
                                        },
                                        "{s_all}"
                                    }
                                    for (id, title, _) in menu {
                                        button {
                                            key: "{id}",
                                            r#type: "button",
                                            class: if category() == id { "pop-item active" } else { "pop-item" },
                                            onmousedown: move |ev| {
                                                ev.prevent_default();
                                                category.set(id.clone());
                                                category_open.set(false);
                                            },
                                            "{title}"
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
