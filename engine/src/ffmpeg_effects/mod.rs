//! The ffmpeg effects catalogue: `effects.json` read into types, and the engine that turns an effect and the values of its
//! controls into ffmpeg arguments. Pure Rust, no window: it belongs in an app's `engine` crate, next to the other modules.
//!
//! ⚠ SOURCE OF TRUTH: the shared ffmpeg-effects folder (`rust/engine`). A copy in an app is never edited there.
//! Twin of `effects-engine.cjs`: the two must write the same text for the same values; `rust/check` proves it.
//!
//! ```ignore
//! let catalog = Catalog::from_json(include_str!("effects.json"))?;
//! let effect = catalog.form("try:gblur").unwrap();
//! let values = defaults(&effect, &catalog.filters)?;   // what the form starts with
//! let args = args(&effect, &catalog.filters, &values, &ctx)?;   // ["-vf", "gblur"]
//! ```

mod compose;
mod engine;
mod expr;
mod model;
mod picker;
mod value;

pub use compose::{compose, Composed, VideoFilter};
pub use engine::{
    args, build, check, choices_of_option, defaults, dur_seconds, escape, find_option, flag_names, flag_set, flag_text, implicit, resolve_controls,
    used_filters, Built, BuiltChain, Choice, Control, Ctx, Dur, Filters, Values,
};
pub use expr::eval_expr;
pub use model::*;
pub use picker::{avail, cmp_version, Avail, Entry, Order};
pub use value::{js_number_string, V};
