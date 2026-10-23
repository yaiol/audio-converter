//! The yaiol site addresses: where an app sends its user (help, download pages).

const SITE: &str = "https://apps.yaiol.com";

/// The localized product page `https://apps.yaiol.com/<lang>/p/<app>/<page>/` (`page` may be empty
/// or nested: `help/releases`). The workspace's language codes use an underscore (`pt_BR`); the web
/// form is a hyphen, so it is converted here, once.
pub fn site_url(app_id: &str, lang: &str, page: &str) -> String {
    let lang = lang.replace('_', "-");
    let path: Vec<&str> = page.split('/').filter(|s| !s.is_empty()).collect();
    if path.is_empty() {
        format!("{SITE}/{lang}/p/{app_id}/")
    } else {
        format!("{SITE}/{lang}/p/{app_id}/{}/", path.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_urls_use_the_web_language_form() {
        assert_eq!(site_url("my-app", "pt_BR", ""), "https://apps.yaiol.com/pt-BR/p/my-app/");
        assert_eq!(site_url("my-app", "en", "help"), "https://apps.yaiol.com/en/p/my-app/help/");
        assert_eq!(
            site_url("my-app", "fr", "/help//releases/"),
            "https://apps.yaiol.com/fr/p/my-app/help/releases/"
        );
    }
}
