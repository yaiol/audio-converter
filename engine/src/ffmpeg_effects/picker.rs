//! What an app's "Add an effect" menu offers: the effects of the catalogue that work on the app's kind of stream, that this system's
//! ffmpeg has, and that are not covered by a hand-written effect. Twin of `offered` / `avail` in `effects-editor-template.html` and
//! `effects-form.js`.

use super::model::*;
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// Is a filter in this system's build?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Avail {
    Yes,
    No,
    /// That system has not been listed yet: nothing says it is missing.
    Unknown,
}

/// Compares two dotted versions number by number.
pub fn cmp_version(a: &str, b: &str) -> Ordering {
    let num = |s: &str| s.split('.').map(|p| p.trim().parse::<i64>().unwrap_or(0)).collect::<Vec<_>>();
    let (x, y) = (num(a), num(b));
    for i in 0..x.len().max(y.len()) {
        let d = x.get(i).copied().unwrap_or(0).cmp(&y.get(i).copied().unwrap_or(0));
        if d != Ordering::Equal {
            return d;
        }
    }
    Ordering::Equal
}

/// `since` and `until` of a filter or an effect, read for one system. `any` is every system.
pub fn avail(since: &BTreeMap<String, Option<String>>, until: &BTreeMap<String, Option<String>>, system: &str, version: Option<&str>) -> Avail {
    if system == "any" {
        return Avail::Yes;
    }
    let since = match since.get(system) {
        None => return Avail::No,
        Some(None) => return Avail::Unknown,
        Some(Some(s)) if s.is_empty() => return Avail::No,
        Some(Some(s)) => s,
    };
    let Some(ver) = version.filter(|v| !v.is_empty()) else { return Avail::Unknown };
    let gone = until.get(system).and_then(|u| u.as_deref()).filter(|u| !u.is_empty()).is_some_and(|u| cmp_version(ver, u) != Ordering::Less);
    if cmp_version(ver, since) != Ordering::Less && !gone { Avail::Yes } else { Avail::No }
}

/// One line of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What identifies the effect: a hand-written effect's id, or `try:` and a filter's name.
    pub key: String,
    pub title: String,
    pub category: String,
    pub level: String,
}

impl Catalog {
    /// The effect a key names: a hand-written effect by its id, or the one a filter is by itself (`try:name`).
    pub fn form(&self, key: &str) -> Option<Effect> {
        match key.strip_prefix("try:") {
            Some(name) => super::engine::implicit(name, &self.filters),
            None => self.effects.get(key).cloned(),
        }
    }

    /// The menu of an app that works on `works` (`video` or `audio`), for one system (`windows`, `linux`, `macos` or `any`).
    /// Not sorted: group by `categories`, then by the title the user's language sorts them under.
    pub fn picker(&self, system: &str, works: &str) -> Vec<Entry> {
        let version = self.systems.get(system).and_then(|s| s.version.as_deref());
        let fits = |w: &str| w == works || w == "both";
        let shown = |a: Avail| a != Avail::No;
        let mut out = Vec::new();
        for (name, f) in &self.filters {
            if f.role != "effect" || !f.covered_by.is_empty() || f.blocked.is_some() || !fits(&f.works) {
                continue;
            }
            let Some(category) = f.category.clone().filter(|c| !c.is_empty()) else { continue };
            if shown(avail(&f.since, &f.until, system, version)) {
                out.push(Entry { key: format!("try:{name}"), title: f.plain.clone().unwrap_or_else(|| name.clone()), category, level: f.level.clone().unwrap_or_default() });
            }
        }
        for (id, e) in &self.effects {
            let Some(category) = e.category.clone().filter(|c| !c.is_empty()) else { continue };
            if fits(&e.works) && shown(avail(&e.since, &e.until, system, version)) {
                out.push(Entry { key: id.clone(), title: e.plain.clone().unwrap_or_else(|| id.clone()), category, level: e.level.clone().unwrap_or_default() });
            }
        }
        out
    }

    /// The effects of an entry, in the order listed, split around the app's size cap: `before` it (what does not want the size final) and
    /// `after` it (what does), each in the order listed; and the problems: an effect that runs before another which changes what it wants.
    /// Twin of `order` in `effects-engine.cjs`. Does not reorder the user's list.
    pub fn order(&self, keys: &[String]) -> Order {
        let traits = |k: &str| -> (&[String], &[String], String) {
            match k.strip_prefix("try:") {
                Some(n) => match self.filters.get(n) {
                    Some(f) => (&f.changes, &f.wants, f.plain.clone().unwrap_or_else(|| n.to_string())),
                    None => (&[], &[], n.to_string()),
                },
                None => match self.effects.get(k) {
                    Some(e) => (&e.changes, &e.wants, e.plain.clone().unwrap_or_else(|| k.to_string())),
                    None => (&[], &[], k.to_string()),
                },
            }
        };
        let wants_size = |k: &String| traits(k).1.iter().any(|w| w == "size");
        let before: Vec<String> = keys.iter().filter(|k| !wants_size(k)).cloned().collect();
        let after: Vec<String> = keys.iter().filter(|k| wants_size(k)).cloned().collect();
        let run: Vec<&String> = before.iter().chain(after.iter()).collect();
        let mut problems = Vec::new();
        for (i, a) in run.iter().enumerate() {
            let (_, wants, a_title) = traits(a);
            for x in wants {
                for b in &run[i + 1..] {
                    let (changes, _, b_title) = traits(b);
                    if changes.iter().any(|c| c == x) {
                        problems.push(format!("{a_title} runs before {b_title}, which changes the {x} it works on"));
                    }
                }
            }
        }
        Order { before, after, problems }
    }
}

/// See `Catalog::order`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Order {
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub problems: Vec<String>,
}
