use super::{h, layout};

pub fn login_page() -> String {
    layout("Sign in", None, &body(None))
}

pub fn login_page_with_error(error: &str) -> String {
    layout("Sign in", None, &body(Some(error)))
}

fn body(error: Option<&str>) -> String {
    let banner = match error {
        Some(msg) => format!(r#"<div class="banner error">{}</div>"#, h(msg)),
        None => String::new(),
    };
    format!(
        r#"<h1>Sign in</h1>
{banner}
<form method="post" action="/login" class="card">
  <div class="field">
    <label for="username">Operator</label>
    <input id="username" name="username" autocomplete="username" autofocus required/>
  </div>
  <div class="field">
    <label for="password">Password</label>
    <input id="password" name="password" type="password" autocomplete="current-password" required/>
  </div>
  <button type="submit">Sign in</button>
</form>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_page_has_form_and_no_error_banner() {
        let out = login_page();
        assert!(out.contains(r#"<form method="post" action="/login""#));
        assert!(!out.contains("banner error"));
    }

    #[test]
    fn login_page_with_error_shows_escaped_message() {
        let out = login_page_with_error("bad <input>");
        assert!(out.contains("banner error"));
        assert!(out.contains("bad &lt;input&gt;"));
        assert!(!out.contains("bad <input>"));
    }
}
