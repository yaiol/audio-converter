//! DropZone — twin of `ui-fx-filedrop.jsx` (`useFileDrop`): a position:relative region that accepts
//! files dropped on it and dims itself with the `.drop-overlay` while one is over it. A component
//! rather than a hook because rsx cannot spread props onto an existing element; the DOM it
//! produces is the same. Not ported yet: the flicker guard on leave (needs `relatedTarget`).

use dioxus::html::HasFileData;
use dioxus::prelude::*;

#[component]
pub fn DropZone(
    class: String,
    label: String,
    on_files: EventHandler<Vec<(String, String)>>,
    children: Element,
) -> Element {
    let mut over = use_signal(|| false);

    rsx! {
        div {
            class: "{class}",
            ondragover: move |e| {
                e.prevent_default();
                over.set(true);
            },
            ondragleave: move |_| over.set(false),
            ondrop: move |e| {
                e.prevent_default();
                e.stop_propagation();
                over.set(false);
                let dropped = e
                    .files()
                    .iter()
                    .map(|f| (f.name(), f.path().display().to_string()))
                    .collect();
                on_files.call(dropped);
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
