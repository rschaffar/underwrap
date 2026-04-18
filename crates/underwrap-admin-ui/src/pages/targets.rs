use super::{attr, h, layout};

pub fn targets_page(operator: &str, targets: &[String]) -> String {
    let rows = if targets.is_empty() {
        String::from(
            r#"<tr><td colspan="2" class="muted">No backend targets configured.</td></tr>"#,
        )
    } else {
        targets
            .iter()
            .map(|t| {
                format!(
                    r#"<tr><td class="mono">{name}</td><td><a class="button" href="/t/{url}">Open</a></td></tr>"#,
                    name = h(t),
                    url = attr(t),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let body = format!(
        r#"<h1>Backend targets</h1>
<p class="muted">Pick a backend to manage its wrapper modes, rules, and grants.</p>
<table>
<thead><tr><th>Target</th><th></th></tr></thead>
<tbody>
{rows}
</tbody>
</table>"#
    );
    layout("Backend targets", Some(operator), &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_page_lists_each_target_and_links_to_it() {
        let out = targets_page("alice", &["corp".to_string(), "personal".to_string()]);
        assert!(out.contains(r#"<a class="button" href="/t/corp">"#));
        assert!(out.contains(r#"<a class="button" href="/t/personal">"#));
    }

    #[test]
    fn targets_page_escapes_target_names() {
        let out = targets_page("alice", &["<evil>".to_string()]);
        assert!(out.contains("&lt;evil&gt;"));
        assert!(!out.contains("href=\"/t/<evil>"));
    }

    #[test]
    fn targets_page_handles_empty_list() {
        let out = targets_page("alice", &[]);
        assert!(out.contains("No backend targets configured"));
    }
}
