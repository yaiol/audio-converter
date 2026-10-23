//! The window's view of the settings: the type and its rules are the engine's (the command line shares
//! them); this adds only which product's folder to read.

pub use engine::settings::{NONE_ID, RunChoice, Settings, max_parallel};

pub fn load() -> Settings {
    engine::settings::load(crate::APP_NAME)
}

pub fn save(settings: &Settings) {
    engine::settings::save(crate::APP_NAME, settings);
}
