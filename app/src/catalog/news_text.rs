// twin: ui-news-text.jsx jsx@d801f7cd744c rsx@73906403f8ba
//! NewsText — twin of `ui-news-text.jsx`: the mini-Markdown renderer for a yaiol news announcement's
//! text. A subset, on purpose: a blank line starts a paragraph, a block whose every line starts with "- "
//! is a bullet list, and inline `[label](http(s) url)`, `**bold**`, `*italic*`. Anything else is plain
//! text, so a stray symbol can never break the dialog. Built as elements, never as HTML. Used by
//! `NewsDialog`. A link opens in the system browser.
//! Differs: none.

use dioxus::prelude::*;

#[derive(Clone, PartialEq, Debug)]
enum Inline {
    Text(String),
    Link { label: String, url: String },
    Bold(String),
    Em(String),
}

/// `[label](http(s)://...)` at the start of `s` -> (label, url, bytes used).
fn link_at(s: &str) -> Option<(String, String, usize)> {
    let rest = s.strip_prefix('[')?;
    let close = rest.find(']')?;
    let label = &rest[..close];
    if label.is_empty() || label.contains('\n') {
        return None;
    }
    let after = rest[close + 1..].strip_prefix('(')?;
    let scheme = if after.starts_with("https://") {
        8
    } else if after.starts_with("http://") {
        7
    } else {
        return None;
    };
    let end = after.find(|c: char| c.is_whitespace() || c == ')')?;
    if !after[end..].starts_with(')') || end <= scheme {
        return None;
    }
    Some((label.to_string(), after[..end].to_string(), 1 + close + 1 + 1 + end + 1))
}

/// `open ... open` around a non-empty run without `*` or a newline -> (inner, bytes used).
fn wrapped_at<'a>(s: &'a str, open: &str) -> Option<(&'a str, usize)> {
    let rest = s.strip_prefix(open)?;
    let end = rest.find(['*', '\n']).unwrap_or(rest.len());
    if end == 0 || !rest[end..].starts_with(open) {
        return None;
    }
    Some((&rest[..end], open.len() + end + open.len()))
}

fn inline(text: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let hit = if rest.starts_with('[') {
            link_at(rest).map(|(label, url, n)| (Inline::Link { label, url }, n))
        } else if rest.starts_with("**") {
            wrapped_at(rest, "**").map(|(s, n)| (Inline::Bold(s.to_string()), n))
        } else if rest.starts_with('*') {
            wrapped_at(rest, "*").map(|(s, n)| (Inline::Em(s.to_string()), n))
        } else {
            None
        };
        match hit {
            Some((node, n)) => {
                if !plain.is_empty() {
                    out.push(Inline::Text(std::mem::take(&mut plain)));
                }
                out.push(node);
                i += n;
            }
            None => {
                let ch = rest.chars().next().unwrap_or(' ');
                plain.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    if !plain.is_empty() {
        out.push(Inline::Text(plain));
    }
    out
}

#[derive(Clone, PartialEq, Debug)]
enum Block {
    Paragraph(Vec<Inline>),
    List(Vec<Vec<Inline>>),
}

/// Split on runs of two or more newlines, as the JSX's `split(/\n{2,}/)`.
fn paragraphs(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            let mut j = i;
            while j < bytes.len() && bytes[j] == b'\n' {
                j += 1;
            }
            if j - i >= 2 {
                out.push(&text[start..i]);
                start = j;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out.push(&text[start..]);
    out
}

fn blocks(text: &str) -> Vec<Block> {
    paragraphs(text)
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .map(|raw| {
            let lines: Vec<&str> = raw.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
            if !lines.is_empty() && lines.iter().all(|l| l.starts_with("- ")) {
                Block::List(lines.iter().map(|l| inline(&l[2..])).collect())
            } else {
                Block::Paragraph(inline(&lines.join(" ")))
            }
        })
        .collect()
}

fn render_inline(nodes: &[Inline]) -> Element {
    rsx! {
        for (i, n) in nodes.iter().enumerate() {
            match n {
                Inline::Text(t) => rsx! { "{t}" },
                Inline::Link { label, url } => {
                    let url = url.clone();
                    rsx! {
                        a {
                            key: "{i}",
                            href: "{url}",
                            onclick: {
                                let url = url.clone();
                                move |e: Event<MouseData>| {
                                    e.prevent_default();
                                    let _ = open::that(&url);
                                }
                            },
                            "{label}"
                        }
                    }
                }
                Inline::Bold(t) => rsx! { b { key: "{i}", "{t}" } },
                Inline::Em(t) => rsx! { em { key: "{i}", "{t}" } },
            }
        }
    }
}

#[component]
pub fn NewsText(text: String) -> Element {
    let parsed = blocks(&text);
    rsx! {
        for (i, b) in parsed.iter().enumerate() {
            match b {
                Block::List(items) => rsx! {
                    ul { key: "{i}", class: "dlp-sub left",
                        for (j, nodes) in items.iter().enumerate() {
                            li { key: "{j}", {render_inline(nodes)} }
                        }
                    }
                },
                Block::Paragraph(nodes) => rsx! {
                    div { key: "{i}", class: "dlp-sub left", {render_inline(nodes)} }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_forms() {
        assert_eq!(inline("a **b** c"), vec![Inline::Text("a ".into()), Inline::Bold("b".into()), Inline::Text(" c".into())]);
        assert_eq!(inline("*x*"), vec![Inline::Em("x".into())]);
        assert_eq!(
            inline("see [site](https://yaiol.com/a) now"),
            vec![
                Inline::Text("see ".into()),
                Inline::Link { label: "site".into(), url: "https://yaiol.com/a".into() },
                Inline::Text(" now".into())
            ]
        );
        // not links / not emphasis: left as text
        assert_eq!(inline("[x](ftp://a)"), vec![Inline::Text("[x](ftp://a)".into())]);
        assert_eq!(inline("a * b"), vec![Inline::Text("a * b".into())]);
        assert_eq!(inline("**open"), vec![Inline::Text("**open".into())]);
    }

    #[test]
    fn block_forms() {
        assert_eq!(blocks("one\ntwo\n\n- a\n- b").len(), 2);
        assert!(matches!(&blocks("- a\n- b")[0], Block::List(l) if l.len() == 2));
        assert!(matches!(&blocks("- a\nb")[0], Block::Paragraph(_)));
        assert_eq!(blocks("\n\n\n").len(), 0);
    }
}
