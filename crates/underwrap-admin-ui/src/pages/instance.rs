use underwrap_protocol::policy::{
    OP_STORE_WRITE, PolicyDocument, PolicyEffect, PolicyGrant, WrapperApprovalMode,
};

use super::{attr, h, layout, url_path};

pub struct InstancePageData<'a> {
    pub operator: &'a str,
    pub target: &'a str,
    pub store: &'a str,
    pub stores: &'a [String],
    pub policy: &'a PolicyDocument,
    pub csrf_token: &'a str,
    pub banner_ok: Option<&'a str>,
    pub banner_error: Option<&'a str>,
}

pub fn instance_page(data: InstancePageData<'_>) -> String {
    let banners = render_banners(data.banner_ok, data.banner_error);
    let store_switcher = render_store_switcher(data.target, data.store, data.stores);
    let wrappers = render_wrappers(data.target, data.store, data.policy, data.csrf_token);
    let rules = render_rules(data.target, data.store, data.policy, data.csrf_token);
    let grants = render_grants(data.target, data.store, data.policy, data.csrf_token);
    let credentials = render_credentials(data.target, data.store, data.policy, data.csrf_token);

    let body = format!(
        r#"{banners}
<h1>Backend: {target}</h1>
{store_switcher}

<h2>Wrappers</h2>
{wrappers}

<h2>Credentials</h2>
{credentials}

<h2>Rules</h2>
{rules}

<h2>Grants</h2>
{grants}"#,
        target = h(data.target),
    );

    layout(
        &format!("Backend {} / {}", data.target, data.store),
        Some(data.operator),
        &body,
    )
}

fn render_banners(ok: Option<&str>, error: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(msg) = ok {
        out.push_str(&format!(r#"<div class="banner ok">{}</div>"#, h(msg)));
    }
    if let Some(msg) = error {
        out.push_str(&format!(r#"<div class="banner error">{}</div>"#, h(msg)));
    }
    out
}

fn render_store_switcher(target: &str, store: &str, stores: &[String]) -> String {
    let options: String = stores
        .iter()
        .map(|s| {
            let selected = if s == store { " selected" } else { "" };
            format!(r#"<option value="{}"{selected}>{}</option>"#, attr(s), h(s))
        })
        .collect();
    format!(
        r#"<form method="get" action="/t/{target}" class="card">
  <div class="field">
    <label>Store</label>
    <select name="store" onchange="this.form.submit()">
{options}
    </select>
  </div>
  <noscript><button type="submit">Switch</button></noscript>
</form>"#,
        target = url_path(target),
    )
}

fn render_wrappers(target: &str, store: &str, policy: &PolicyDocument, csrf: &str) -> String {
    if policy.wrappers.is_empty() {
        return r#"<p class="muted">No wrapper states yet. Set one with the form below any new wrapper.</p>
<form method="post" action="" class="card muted">
  <p>Add a wrapper state by setting a rule or using the CLI; the wrapper mode editor appears once a wrapper state exists.</p>
</form>"#
            .to_string();
    }
    let rows: Vec<String> = policy
        .wrappers
        .iter()
        .map(|w| {
            let (tag_class, tag_label) = match &w.mode {
                WrapperApprovalMode::Locked => ("tag locked", "locked".to_string()),
                WrapperApprovalMode::Unlocked { until_epoch: None } => {
                    ("tag unlocked", "unlocked".to_string())
                }
                WrapperApprovalMode::Unlocked {
                    until_epoch: Some(t),
                } => ("tag unlocked", format!("unlocked until {t}")),
                WrapperApprovalMode::PerCallApproval { .. } => {
                    ("tag per-call", "per-call approval".to_string())
                }
            };
            let reason = w.reason.as_deref().unwrap_or("");
            let approval_key_value = match &w.mode {
                WrapperApprovalMode::PerCallApproval { approval_key } => approval_key.as_str(),
                _ => "",
            };
            let until_value = match &w.mode {
                WrapperApprovalMode::Unlocked {
                    until_epoch: Some(t),
                } => t.to_string(),
                _ => String::new(),
            };
            format!(
                r#"<tr>
  <td class="mono">{name}</td>
  <td><span class="{tag_class}">{tag_label}</span></td>
  <td class="muted">{reason}</td>
</tr>
<tr><td colspan="3">
<form method="post" action="/t/{target}/s/{store}/wrapper/{name_attr}/mode" class="card">
  <input type="hidden" name="csrf_token" value="{csrf}"/>
  <div class="field">
    <label>Change mode</label>
    <select name="mode">
      <option value="locked">locked</option>
      <option value="unlocked">unlocked (permanent)</option>
      <option value="unlocked_until">unlocked until…</option>
      <option value="per_call_approval">per-call approval</option>
    </select>
  </div>
  <div class="field">
    <label>until_epoch (for unlocked_until)</label>
    <input name="until_epoch" value="{until_value}"/>
  </div>
  <div class="field">
    <label>approval_key (OpenSSH format, for per_call_approval)</label>
    <textarea name="approval_key" rows="2">{approval_key}</textarea>
  </div>
  <div class="field">
    <label>reason (optional, shown to operators)</label>
    <input name="reason"/>
  </div>
  <button type="submit">Apply mode</button>
</form>
</td></tr>"#,
                name = h(&w.wrapper),
                name_attr = url_path(&w.wrapper),
                target = url_path(target),
                store = url_path(store),
                csrf = attr(csrf),
                reason = h(reason),
                tag_label = h(&tag_label),
                until_value = attr(&until_value),
                approval_key = h(approval_key_value),
            )
        })
        .collect();
    format!(
        r#"<table>
<thead><tr><th>wrapper</th><th>mode</th><th>reason</th></tr></thead>
<tbody>
{}
</tbody>
</table>"#,
        rows.join("\n")
    )
}

fn render_rules(target: &str, store: &str, policy: &PolicyDocument, csrf: &str) -> String {
    let rows: Vec<String> = policy
        .rules
        .iter()
        .map(|r| {
            format!(
                r#"<tr>
  <td class="mono">{id}</td>
  <td class="mono">{wrapper}</td>
  <td class="mono">{resource}</td>
  <td class="mono">{operation}</td>
  <td>{effect}</td>
  <td>
    <form method="post" action="/t/{target}/s/{store}/rule/{id_attr}/delete" class="inline-form">
      <input type="hidden" name="csrf_token" value="{csrf}"/>
      <button type="submit" class="danger">delete</button>
    </form>
  </td>
</tr>"#,
                id = h(&r.id),
                id_attr = url_path(&r.id),
                wrapper = h(&r.wrapper),
                resource = h(&r.resource),
                operation = h(&r.operation),
                effect = render_effect(&r.effect),
                target = url_path(target),
                store = url_path(store),
                csrf = attr(csrf),
            )
        })
        .collect();
    format!(
        r#"<table>
<thead><tr><th>id</th><th>wrapper</th><th>resource</th><th>operation</th><th>effect</th><th></th></tr></thead>
<tbody>
{rows}
</tbody>
</table>
<form method="post" action="/t/{target}/s/{store}/rule/upsert" class="card">
  <input type="hidden" name="csrf_token" value="{csrf}"/>
  <h2>Add rule</h2>
  <div class="field"><label>id</label><input name="id" required/></div>
  <div class="field"><label>wrapper</label><input name="wrapper" required/></div>
  <div class="field"><label>resource (e.g. secret:github/token)</label><input name="resource" required/></div>
  <div class="field"><label>operation (e.g. secret.read)</label><input name="operation" required/></div>
  <div class="field">
    <label>effect</label>
    <select name="effect">
      <option value="allow">allow</option>
      <option value="deny">deny</option>
    </select>
  </div>
  <button type="submit">Add rule</button>
</form>"#,
        rows = rows.join("\n"),
        target = url_path(target),
        store = url_path(store),
        csrf = attr(csrf),
    )
}

fn render_grants(target: &str, store: &str, policy: &PolicyDocument, csrf: &str) -> String {
    let rows: Vec<String> = policy
        .grants
        .iter()
        .map(|g| {
            let mut window = match (g.starts_at_epoch, g.expires_at_epoch) {
                (Some(s), Some(e)) => format!("{s} → {e}"),
                (None, Some(e)) => format!("until {e}"),
                (Some(s), None) => format!("from {s}"),
                _ => "always".to_string(),
            };
            if let Some(reason) = &g.reason {
                window.push_str(&format!(" — {reason}"));
            }
            format!(
                r#"<tr>
  <td class="mono">{id}</td>
  <td class="mono">{wrapper}</td>
  <td class="mono">{resource}</td>
  <td class="mono">{operation}</td>
  <td>{effect}</td>
  <td class="muted">{window}</td>
  <td>
    <form method="post" action="/t/{target}/s/{store}/grant/{id_attr}/delete" class="inline-form">
      <input type="hidden" name="csrf_token" value="{csrf}"/>
      <button type="submit" class="danger">delete</button>
    </form>
  </td>
</tr>"#,
                id = h(&g.id),
                id_attr = url_path(&g.id),
                wrapper = h(&g.wrapper),
                resource = h(&g.resource),
                operation = h(&g.operation),
                effect = render_effect(&g.effect),
                window = h(&window),
                target = url_path(target),
                store = url_path(store),
                csrf = attr(csrf),
            )
        })
        .collect();
    format!(
        r#"<table>
<thead><tr><th>id</th><th>wrapper</th><th>resource</th><th>operation</th><th>effect</th><th>window</th><th></th></tr></thead>
<tbody>
{rows}
</tbody>
</table>
<form method="post" action="/t/{target}/s/{store}/grant/upsert" class="card">
  <input type="hidden" name="csrf_token" value="{csrf}"/>
  <h2>Add grant</h2>
  <div class="field"><label>id</label><input name="id" required/></div>
  <div class="field"><label>wrapper</label><input name="wrapper" required/></div>
  <div class="field"><label>resource</label><input name="resource" required/></div>
  <div class="field"><label>operation</label><input name="operation" required/></div>
  <div class="field">
    <label>effect</label>
    <select name="effect">
      <option value="allow">allow</option>
      <option value="deny">deny</option>
    </select>
  </div>
  <div class="field"><label>starts_at_epoch (optional)</label><input name="starts_at_epoch"/></div>
  <div class="field"><label>expires_at_epoch (optional)</label><input name="expires_at_epoch"/></div>
  <div class="field"><label>reason (optional)</label><input name="reason"/></div>
  <button type="submit">Add grant</button>
</form>"#,
        rows = rows.join("\n"),
        target = url_path(target),
        store = url_path(store),
        csrf = attr(csrf),
    )
}

fn render_credentials(target: &str, store: &str, policy: &PolicyDocument, csrf: &str) -> String {
    let active_rows = render_active_mutation_windows(target, store, policy, csrf);
    let enable_form = format!(
        r#"<form method="post" action="/t/{target}/s/{store}/mutation/enable" class="card">
  <input type="hidden" name="csrf_token" value="{csrf}"/>
  <h3>Allow mutation</h3>
  <p class="muted">Creates a time-bounded allow grant for <span class="mono">store.write</span> on <span class="mono">cred:&lt;secret_id&gt;</span> for the named wrapper. A <span class="mono">/v1/admin/secret/write</span> for that tuple succeeds while the grant is active.</p>
  <div class="field"><label>wrapper</label><input name="wrapper" required/></div>
  <div class="field"><label>secret_id (e.g. github/token)</label><input name="secret_id" required/></div>
  <div class="field">
    <label>duration</label>
    <select name="duration">
      <option value="30m">30 minutes (default)</option>
      <option value="10m">10 minutes</option>
      <option value="2h">2 hours</option>
      <option value="permanent">permanent (requires confirmation)</option>
    </select>
  </div>
  <div class="field"><label>reason (optional, shown in grants)</label><input name="reason"/></div>
  <div class="field">
    <label>
      <input type="checkbox" name="confirm_permanent" value="yes"/>
      I understand: permanent allow-grants are operationally equivalent to <span class="mono">mutable: true</span>.
    </label>
  </div>
  <button type="submit">Allow mutation</button>
</form>"#,
        target = url_path(target),
        store = url_path(store),
        csrf = attr(csrf),
    );
    let write_form = format!(
        r#"<form method="post" action="/t/{target}/s/{store}/credential/write" class="card">
  <input type="hidden" name="csrf_token" value="{csrf}"/>
  <h3>Set credential value</h3>
  <p class="muted">Succeeds only while an allow grant for <span class="mono">(wrapper, cred:&lt;secret_id&gt;, store.write)</span> is active.</p>
  <div class="field"><label>wrapper</label><input name="wrapper" required/></div>
  <div class="field"><label>secret_id</label><input name="secret_id" required/></div>
  <div class="field"><label>value (raw, not base64)</label><textarea name="value" rows="3" required></textarea></div>
  <button type="submit">Write value</button>
</form>"#,
        target = url_path(target),
        store = url_path(store),
        csrf = attr(csrf),
    );
    format!("{active_rows}\n{enable_form}\n{write_form}")
}

fn render_active_mutation_windows(
    target: &str,
    store: &str,
    policy: &PolicyDocument,
    csrf: &str,
) -> String {
    let windows: Vec<&PolicyGrant> = policy
        .grants
        .iter()
        .filter(|g| g.operation == OP_STORE_WRITE && g.effect == PolicyEffect::Allow)
        .collect();
    if windows.is_empty() {
        return r#"<p class="muted">No active mutation windows. Use the form below to allow writes to a credential for a bounded time.</p>"#
            .to_string();
    }
    let rows: Vec<String> = windows
        .iter()
        .map(|g| {
            let secret_id = g.resource.strip_prefix("cred:").unwrap_or(&g.resource);
            let window = match (g.starts_at_epoch, g.expires_at_epoch) {
                (Some(s), Some(e)) => format!("{s} → {e}"),
                (None, Some(e)) => format!("until {e}"),
                (Some(s), None) => format!("from {s} (permanent)"),
                _ => "permanent".to_string(),
            };
            let reason = g
                .reason
                .as_deref()
                .map(|r| format!(" — {}", h(r)))
                .unwrap_or_default();
            format!(
                r#"<tr>
  <td class="mono">{wrapper}</td>
  <td class="mono">{secret_id}</td>
  <td class="muted">{window}{reason}</td>
  <td>
    <form method="post" action="/t/{target}/s/{store}/mutation/revoke" class="inline-form">
      <input type="hidden" name="csrf_token" value="{csrf}"/>
      <input type="hidden" name="wrapper" value="{wrapper_attr}"/>
      <input type="hidden" name="secret_id" value="{secret_id_attr}"/>
      <input type="hidden" name="grant_id" value="{grant_id_attr}"/>
      <button type="submit" class="danger">revoke</button>
    </form>
  </td>
</tr>"#,
                wrapper = h(&g.wrapper),
                wrapper_attr = attr(&g.wrapper),
                secret_id = h(secret_id),
                secret_id_attr = attr(secret_id),
                grant_id_attr = attr(&g.id),
                window = h(&window),
                reason = reason,
                target = url_path(target),
                store = url_path(store),
                csrf = attr(csrf),
            )
        })
        .collect();
    format!(
        r#"<h3>Active mutation windows</h3>
<table>
<thead><tr><th>wrapper</th><th>secret_id</th><th>window</th><th></th></tr></thead>
<tbody>
{rows}
</tbody>
</table>"#,
        rows = rows.join("\n"),
    )
}

fn render_effect(effect: &PolicyEffect) -> String {
    match effect {
        PolicyEffect::Allow => r#"<span class="tag unlocked">allow</span>"#.to_string(),
        PolicyEffect::Deny => r#"<span class="tag locked">deny</span>"#.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use underwrap_protocol::policy::{PolicyGrant, PolicyRule, WrapperPolicyState};

    fn sample_policy() -> PolicyDocument {
        PolicyDocument {
            wrappers: vec![
                WrapperPolicyState {
                    wrapper: "gh-authorized".to_string(),
                    mode: WrapperApprovalMode::Unlocked { until_epoch: None },
                    reason: None,
                },
                WrapperPolicyState {
                    wrapper: "aws-prod".to_string(),
                    mode: WrapperApprovalMode::PerCallApproval {
                        approval_key: "ssh-ed25519 AAAA".to_string(),
                    },
                    reason: Some("ops approval required".to_string()),
                },
            ],
            rules: vec![PolicyRule {
                id: "r1".to_string(),
                wrapper: "gh-authorized".to_string(),
                resource: "secret:github/token".to_string(),
                operation: "secret.read".to_string(),
                effect: PolicyEffect::Allow,
            }],
            grants: vec![PolicyGrant {
                id: "g1".to_string(),
                wrapper: "aws-prod".to_string(),
                resource: "secret:aws/prod".to_string(),
                operation: "secret.read".to_string(),
                effect: PolicyEffect::Allow,
                starts_at_epoch: Some(10),
                expires_at_epoch: Some(20),
                reason: Some("one-time deploy".to_string()),
            }],
        }
    }

    #[test]
    fn instance_page_shows_wrapper_rule_and_grant_rows() {
        let policy = sample_policy();
        let html = instance_page(InstancePageData {
            operator: "alice",
            target: "corp",
            store: "work",
            stores: &["work".to_string(), "personal".to_string()],
            policy: &policy,
            csrf_token: "CSRF123",
            banner_ok: Some("saved"),
            banner_error: None,
        });
        assert!(html.contains("gh-authorized"));
        assert!(html.contains("aws-prod"));
        assert!(html.contains("per-call approval"));
        assert!(html.contains("secret:github/token"));
        assert!(html.contains("one-time deploy"));
        assert!(html.contains("CSRF123"));
        assert!(html.contains(r#"<div class="banner ok">saved</div>"#));
        // Store selector has the current store marked selected.
        assert!(html.contains(r#"<option value="work" selected>"#));
    }

    #[test]
    fn credentials_section_renders_forms_and_active_windows() {
        let policy = PolicyDocument {
            wrappers: Vec::new(),
            rules: Vec::new(),
            grants: vec![PolicyGrant {
                id: "mutation:gh-authorized:github/token".to_string(),
                wrapper: "gh-authorized".to_string(),
                resource: "cred:github/token".to_string(),
                operation: OP_STORE_WRITE.to_string(),
                effect: PolicyEffect::Allow,
                starts_at_epoch: None,
                expires_at_epoch: Some(1_700_000_000),
                reason: Some("ticket IT-42".to_string()),
            }],
        };
        let html = instance_page(InstancePageData {
            operator: "alice",
            target: "corp",
            store: "work",
            stores: &["work".to_string()],
            policy: &policy,
            csrf_token: "CSRF",
            banner_ok: None,
            banner_error: None,
        });

        // Section header + both forms are present.
        assert!(html.contains("<h2>Credentials</h2>"), "header missing");
        assert!(
            html.contains(r#"action="/t/corp/s/work/mutation/enable""#),
            "enable form missing",
        );
        assert!(
            html.contains(r#"action="/t/corp/s/work/credential/write""#),
            "write form missing",
        );
        assert!(
            html.contains(r#"action="/t/corp/s/work/mutation/revoke""#),
            "revoke form missing in active windows row",
        );
        // The existing mutation grant surfaces in the "Active mutation windows" table
        // with the secret_id stripped from the `cred:` prefix.
        assert!(html.contains("Active mutation windows"));
        assert!(html.contains(">github/token<"));
        assert!(html.contains("until 1700000000"));
        assert!(html.contains("ticket IT-42"));
        // Confirmation checkbox is rendered for permanent grants.
        assert!(html.contains(r#"name="confirm_permanent""#));
    }

    #[test]
    fn credentials_section_shows_placeholder_when_no_mutation_grants() {
        let policy = PolicyDocument::default();
        let html = instance_page(InstancePageData {
            operator: "alice",
            target: "corp",
            store: "work",
            stores: &["work".to_string()],
            policy: &policy,
            csrf_token: "CSRF",
            banner_ok: None,
            banner_error: None,
        });
        assert!(html.contains("No active mutation windows"));
        assert!(
            !html.contains("Active mutation windows"),
            "header only shows when there are rows",
        );
    }

    #[test]
    fn instance_page_escapes_policy_content() {
        let policy = PolicyDocument {
            wrappers: vec![WrapperPolicyState {
                wrapper: "<script>".to_string(),
                mode: WrapperApprovalMode::Locked,
                reason: Some("oops \"injected\"".to_string()),
            }],
            rules: Vec::new(),
            grants: Vec::new(),
        };
        let html = instance_page(InstancePageData {
            operator: "alice",
            target: "corp",
            store: "work",
            stores: &["work".to_string()],
            policy: &policy,
            csrf_token: "CSRF",
            banner_ok: None,
            banner_error: None,
        });
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&quot;injected&quot;"));
    }
}
