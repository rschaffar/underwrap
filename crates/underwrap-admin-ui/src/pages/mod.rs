//! Server-rendered HTML for the admin UI.
//!
//! The pages are built as plain strings. HTML-interpolated values go through
//! [`h`] for escaping, attribute values through [`attr`]. The output is
//! static: all interactivity happens via form submissions.

mod instance;
mod login;
mod targets;

pub use instance::{InstancePageData, instance_page};
pub use login::{login_page, login_page_with_error};
pub use targets::targets_page;

const STYLE: &str = include_str!("../assets/style.css");

/// HTML-escape a string for use inside element text.
pub fn h(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// HTML-escape a string for use inside a double-quoted attribute value.
/// Same as `h` — the attribute values we emit are always double-quoted.
pub fn attr(s: &str) -> String {
    h(s)
}

/// Shared chrome wrapping every page.
pub fn layout(title: &str, operator: Option<&str>, body: &str) -> String {
    let title_html = h(&format!("{title} — underwrap admin"));
    let nav = match operator {
        Some(op) => format!(
            r#"<nav>
                <span class="operator">{}</span>
                <form method="post" action="/logout" class="inline">
                    <button type="submit" class="link">log out</button>
                </form>
            </nav>"#,
            h(op)
        ),
        None => "<nav></nav>".to_string(),
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>{title_html}</title>
<style>{STYLE}</style>
</head>
<body>
<header class="topbar">
<a class="brand" href="/">underwrap admin</a>
{nav}
</header>
<main>
{body}
</main>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h_escapes_html_metacharacters() {
        assert_eq!(h("a<b>&\"'"), "a&lt;b&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn layout_renders_operator_and_title() {
        let out = layout("Home", Some("alice"), "<p>hi</p>");
        assert!(out.contains("<title>Home — underwrap admin</title>"));
        assert!(out.contains(r#"<span class="operator">alice</span>"#));
        assert!(out.contains("<p>hi</p>"));
    }

    #[test]
    fn layout_without_operator_shows_no_logout() {
        let out = layout("Login", None, "<p>hi</p>");
        assert!(!out.contains("log out"));
    }
}
