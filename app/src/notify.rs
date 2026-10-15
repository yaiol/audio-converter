//! The operating-system notice shown when a run of conversions ends.
//!
//! There is no setting for it: Windows (Settings > Notifications) and the other systems list the
//! app by name and let the user mute it there.

/// Show one notice. Never fails the caller: a system that cannot show it just shows nothing.
pub fn show(title: &str, body: &str) {
    let mut n = notify_rust::Notification::new();
    n.summary(title).body(body);
    #[cfg(windows)]
    {
        n.app_id(&register());
    }
    #[cfg(not(windows))]
    {
        n.appname(env!("YAIOL_PRODUCT_NAME"));
    }
    let _ = n.show();
}

/// An unpackaged Windows program has no identity of its own, so its toasts would carry the name of
/// the program that sent them. Registering an id with a display name under the user's classes key
/// is the documented light way to give it one; it is written once per run and needs no rights.
#[cfg(windows)]
fn register() -> String {
    use std::os::windows::process::CommandExt;
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(|| {
        let id = format!("com.yaiol.{}", env!("CARGO_PKG_NAME"));
        let key = ["HKCU", "Software", "Classes", "AppUserModelId", id.as_str()].join("\\");
        let name = format!("yaiol {}", env!("YAIOL_PRODUCT_NAME"));
        let _ = std::process::Command::new("reg")
            .args(["add", &key, "/v", "DisplayName", "/t", "REG_SZ", "/d", &name, "/f"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output();
        id
    })
    .clone()
}
