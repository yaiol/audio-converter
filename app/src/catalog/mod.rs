//! The yaiol UI catalog, Dioxus implementation.
//!
//! One catalog, two implementations: the CSS in `assets/` is the shared contract (copied from the
//! shared catalog, never edited here), and each control below emits the SAME class names and
//! element structure as its JSX twin in the shared catalog. Only the controls this app uses are
//! ported; the rest are `pending` — see the project's CLAUDE.md for the status table.

mod equalizer;
mod filedrop;
mod header;
mod icons;
mod numberfield;
pub mod news;
mod news_dialog;
mod news_loader;
mod news_setting;
mod news_text;
mod settings;

pub use equalizer::Equalizer;
pub use filedrop::DropZone;
pub use header::AppHeader;
pub use icons::GithubIcon;
pub use news_loader::NewsLoader;
pub use news_setting::NewsSetting;
pub use numberfield::NumberField as NumberInput;
pub use settings::{AppTab, SettingsView};
