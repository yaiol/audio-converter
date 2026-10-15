//! Lucide glyphs the `dioxus-free-icons` release does not ship yet, drawn the way it draws its own
//! (same view box, stroke and caps) so they sit in the same buttons without a size override.

use dioxus::prelude::*;
use dioxus_free_icons::IconShape;

/// lucide `brush-cleaning`.
#[derive(Clone, Copy, PartialEq)]
pub struct LdBrushCleaning;

impl IconShape for LdBrushCleaning {
    fn view_box(&self) -> &str {
        "0 0 24 24"
    }
    fn xmlns(&self) -> &str {
        "http://www.w3.org/2000/svg"
    }
    fn fill_and_stroke<'a>(&self, user_color: &'a str) -> (&'a str, &'a str, &'a str) {
        ("none", user_color, "2")
    }
    fn stroke_linecap(&self) -> &str {
        "round"
    }
    fn stroke_linejoin(&self) -> &str {
        "round"
    }
    fn child_elements(&self) -> Element {
        rsx! {
            path { d: "m16 22-1-4" }
            path { d: "M19 14a1 1 0 0 0 1-1v-1a2 2 0 0 0-2-2h-3a1 1 0 0 1-1-1V4a2 2 0 0 0-4 0v5a1 1 0 0 1-1 1H6a2 2 0 0 0-2 2v1a1 1 0 0 0 1 1" }
            path { d: "M19 14H5l-1.973 6.767A1 1 0 0 0 4 22h16a1 1 0 0 0 .973-1.233z" }
            path { d: "m8 22 1-4" }
        }
    }
}
