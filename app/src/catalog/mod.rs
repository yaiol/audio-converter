//! The yaiol UI catalog, Dioxus implementation.
//!
//! One catalog, two implementations: the CSS in `assets/` is the shared contract (copied from the
//! shared catalog, never edited here), and each control below emits the SAME class names and
//! element structure as its JSX twin in the shared catalog. Only the controls this app uses are
//! ported; the rest are `pending` — see the project's CLAUDE.md for the status table.

mod filedrop;
mod header;
mod icons;
mod knob;
mod numberfield;
mod settings;
mod update_banner;

pub use filedrop::DropZone;
pub use header::AppHeader;
pub use icons::GithubIcon;
pub use knob::Knob;
pub use numberfield::NumberField as NumberInput;
pub use settings::{AppTab, SettingsView};
pub use update_banner::UpdateBanner;
