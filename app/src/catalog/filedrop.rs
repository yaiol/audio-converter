// twin: ui-fx-filedrop.jsx jsx@72a3a2fbfa64 rsx@ab19e5f2f472
//! DropZone — twin of `ui-fx-filedrop.jsx` (`useFileDrop`): a position:relative region that accepts files
//! dropped on it and dims itself with the `.drop-overlay` while one is over it. A component rather than a
//! hook because rsx cannot spread props onto an existing element; the DOM it produces is the same.
//!
//! `accept` is the list of extensions to take (case-insensitive; empty = everything); files that do not
//! match are ignored. The overlay stays up while the pointer crosses between the region\'s children and is
//! cleared only when it leaves the region entirely - the JSX asks the event where the pointer went next
//! (`relatedTarget`), which Dioxus does not expose, so the same guarantee is kept by counting how many
//! drag-enters are still unmatched by a drag-leave.
//! Differs: behaviour none. Form only: a dropped file is `(name, path)` where the JSX hands a `File` object (a Rust
//! app wants the path, not the bytes); `on_file` (the first matching file) and `on_files` (every matching file) are
//! the JSX's `onFile` / `onFiles`.

use dioxus::html::HasFileData;
use dioxus::prelude::*;

#[component]
pub fn DropZone(
    class: String,
    label: String,
    #[props(default)] accept: Vec<String>,
    /// The first matching file (the common "open a document" case), as `(name, path)`.
    #[props(default)] on_file: Option<EventHandler<(String, String)>>,
    /// Every matching file.
    #[props(default)] on_files: Option<EventHandler<Vec<(String, String)>>>,
    children: Element,
) -> Element {
    let mut over = use_signal(|| false);
    // Drag-enters not yet matched by a drag-leave: the overlay goes only when this is back to zero.
    let mut depth = use_signal(|| 0i32);
    let exts: Vec<String> = accept.iter().map(|e| e.to_lowercase()).collect();

    rsx! {
        div {
            class: "{class}",
            ondragenter: move |e| {
                e.prevent_default();
                depth += 1;
                over.set(true);
            },
            ondragover: move |e| {
                e.prevent_default();
                over.set(true);
            },
            ondragleave: move |_| {
                depth -= 1;
                if depth() <= 0 {
                    depth.set(0);
                    over.set(false);
                }
            },
            ondrop: move |e| {
                e.prevent_default();
                e.stop_propagation();
                depth.set(0);
                over.set(false);
                let dropped: Vec<(String, String)> = e
                    .files()
                    .iter()
                    .filter(|f| exts.is_empty() || exts.iter().any(|x| f.name().to_lowercase().ends_with(x)))
                    .map(|f| (f.name(), f.path().display().to_string()))
                    .collect();
                if let Some(first) = dropped.first().cloned() {
                    if let Some(h) = on_files {
                        h.call(dropped);
                    }
                    if let Some(h) = on_file {
                        h.call(first);
                    }
                }
            },
            {children}
            if over() {
                div { class: "drop-overlay",
                    span { class: "drop-overlay-label", "{label}" }
                }
            }
        }
    }
}
