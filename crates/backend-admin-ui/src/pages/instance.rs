use std::collections::{BTreeMap, BTreeSet};

use underwrap_protocol::policy::{PolicyDocument, PolicyEffect, WrapperApprovalMode};
use underwrap_protocol::service::CatalogEntry;

use super::{attr, h, layout, url_path};

pub struct InstancePageData<'a> {
    pub operator: &'a str,
    pub target: &'a str,
    pub store: &'a str,
    pub stores: &'a [String],
    pub policy: &'a PolicyDocument,
    pub catalog: &'a [CatalogEntry],
    pub secret_ids: &'a [String],
    pub selected_resource: Option<&'a str>,
    pub csrf_token: &'a str,
    pub banner_ok: Option<&'a str>,
    pub banner_error: Option<&'a str>,
}

const OPERATIONS: [&str; 5] = [
    "env.read",
    "files.read",
    "files.write",
    "libsecret.read",
    "libsecret.write",
];

pub fn instance_page(data: InstancePageData<'_>) -> String {
    let banners = render_banners(data.banner_ok, data.banner_error);
    let store_switcher = render_store_switcher(data.target, data.store, data.stores);

    let resources = collect_resources(data.catalog, data.secret_ids);
    let selected_resource = data
        .selected_resource
        .filter(|r| resources.contains(&r.to_string()))
        .map(ToString::to_string)
        .or_else(|| resources.first().cloned());

    let resource_selector = render_resource_selector(
        data.target,
        data.store,
        data.stores,
        &resources,
        selected_resource.as_deref(),
    );
    let resource_editor = render_resource_editor(
        data.target,
        data.store,
        data.secret_ids,
        selected_resource.as_deref(),
        data.csrf_token,
    );
    let matrix = render_access_matrix(
        data.target,
        data.store,
        data.policy,
        data.catalog,
        selected_resource.as_deref(),
        data.csrf_token,
    );
    let wrappers = render_wrappers(data.target, data.store, data.policy, data.csrf_token);

    let body = format!(
        r#"{banners}
<h1>Backend: {target}</h1>
{store_switcher}

<h2>Resources</h2>
{resource_selector}
{resource_editor}

<h2>Access matrix</h2>
{matrix}

<h2>Wrappers</h2>
{wrappers}"#,
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
    <select name="store">
{options}
    </select>
  </div>
  <button type="submit">Switch store</button>
</form>"#,
        target = url_path(target),
    )
}

fn collect_resources(catalog: &[CatalogEntry], secret_ids: &[String]) -> Vec<String> {
    let mut out = BTreeSet::new();
    for entry in catalog {
        out.insert(entry.resource.clone());
    }
    for secret_id in secret_ids {
        if let Some((resource, _, _)) = split_resource_id(secret_id) {
            out.insert(resource.to_string());
        }
    }
    out.into_iter().collect()
}

fn render_resource_selector(
    target: &str,
    store: &str,
    stores: &[String],
    resources: &[String],
    selected_resource: Option<&str>,
) -> String {
    let options: String = resources
        .iter()
        .map(|resource| {
            let selected = if selected_resource == Some(resource.as_str()) {
                " selected"
            } else {
                ""
            };
            format!(
                r#"<option value="{}"{selected}>{}</option>"#,
                attr(resource),
                h(resource)
            )
        })
        .collect();
    let store_options: String = stores
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
    <select name="store">
{store_options}
    </select>
  </div>
  <div class="field">
    <label>Resource</label>
    <select name="resource">
{options}
    </select>
  </div>
  <button type="submit">Open resource</button>
</form>"#,
        target = url_path(target),
        store_options = store_options,
        options = if resources.is_empty() {
            r#"      <option value="">(no resources registered yet)</option>"#.to_string()
        } else {
            options
        },
    )
}

#[derive(Default)]
struct ResourceView {
    env_vars: Vec<String>,
    files: Vec<String>,
    libsecret_items: Vec<String>,
}

fn split_resource_id(secret_id: &str) -> Option<(&str, &str, &str)> {
    for slot in ["/env/", "/files/", "/libsecret/"] {
        if let Some((resource, rest)) = secret_id.split_once(slot) {
            let slot_name = &slot[1..slot.len() - 1];
            return Some((resource, slot_name, rest));
        }
    }
    None
}

fn render_resource_editor(
    target: &str,
    store: &str,
    secret_ids: &[String],
    selected_resource: Option<&str>,
    csrf: &str,
) -> String {
    let Some(selected_resource) = selected_resource else {
        return r#"<p class="muted">No registered resources yet. Restart a frontend service to register wrapper/resource combinations.</p>"#.to_string();
    };
    let mut resources: BTreeMap<String, ResourceView> = BTreeMap::new();
    for secret_id in secret_ids {
        if let Some((resource_id, slot, item)) = split_resource_id(secret_id) {
            let entry = resources.entry(resource_id.to_string()).or_default();
            match slot {
                "env" => entry.env_vars.push(item.to_string()),
                "files" => entry.files.push(item.to_string()),
                "libsecret" => entry.libsecret_items.push(item.to_string()),
                _ => {}
            }
        }
    }
    let view = resources.remove(selected_resource).unwrap_or_default();
    format!(
        r#"<div class="card">
  <h3 class="mono">{resource}</h3>
  <div class="grid three">
    <div>
      <h4>env</h4>
      {env_rows}
    </div>
    <div>
      <h4>files</h4>
      {file_rows}
    </div>
    <div>
      <h4>libsecret</h4>
      {libsecret_rows}
    </div>
  </div>
</div>
<div class="grid two">
  {env_form}
  {files_form}
</div>
<div class="grid one">
  {libsecret_form}
</div>"#,
        resource = h(selected_resource),
        env_rows = render_slot_rows(target, store, selected_resource, "env", &view.env_vars, csrf),
        file_rows = render_slot_rows(target, store, selected_resource, "files", &view.files, csrf),
        libsecret_rows = render_slot_rows(target, store, selected_resource, "libsecret", &view.libsecret_items, csrf),
        env_form = render_env_write_form(target, store, selected_resource, csrf),
        files_form = render_files_write_form(target, store, selected_resource, csrf),
        libsecret_form = render_libsecret_write_form(target, store, selected_resource, csrf),
    )
}

fn render_slot_rows(
    target: &str,
    store: &str,
    resource_id: &str,
    slot: &str,
    items: &[String],
    csrf: &str,
) -> String {
    if items.is_empty() {
        return r#"<p class="muted">empty</p>"#.to_string();
    }
    let rows: Vec<String> = items
        .iter()
        .map(|item| {
            let secret_id = format!("{resource_id}/{slot}/{item}");
            format!(
                r#"<div class="row between"><span class="mono">{item}</span>
<form method="post" action="/t/{target}/s/{store}/secret/delete" class="inline-form">
  <input type="hidden" name="csrf_token" value="{csrf}"/>
  <input type="hidden" name="secret_id" value="{secret_id_attr}"/>
  <button type="submit" class="danger">delete</button>
</form></div>"#,
                item = h(item),
                target = url_path(target),
                store = url_path(store),
                csrf = attr(csrf),
                secret_id_attr = attr(&secret_id),
            )
        })
        .collect();
    rows.join("\n")
}

fn render_env_write_form(target: &str, store: &str, resource_id: &str, csrf: &str) -> String {
    format!(
        r#"<form method="post" action="/t/{target}/s/{store}/secret/write-env" class="card">
  <input type="hidden" name="csrf_token" value="{csrf}"/>
  <input type="hidden" name="resource_id" value="{resource_attr}"/>
  <h3>Set env slot</h3>
  <div class="field"><label>env var name</label><input name="var_name" required/></div>
  <div class="field"><label>value</label><textarea name="value" rows="3" required></textarea></div>
  <button type="submit">Write env value</button>
</form>"#,
        target = url_path(target),
        store = url_path(store),
        csrf = attr(csrf),
        resource_attr = attr(resource_id),
    )
}

fn render_files_write_form(target: &str, store: &str, resource_id: &str, csrf: &str) -> String {
    format!(
        r#"<form method="post" action="/t/{target}/s/{store}/secret/write-file" class="card">
  <input type="hidden" name="csrf_token" value="{csrf}"/>
  <input type="hidden" name="resource_id" value="{resource_attr}"/>
  <h3>Set files slot entry</h3>
  <div class="field"><label>relative file path</label><input name="path" required/></div>
  <div class="field"><label>content</label><textarea name="value" rows="6" required></textarea></div>
  <button type="submit">Write file entry</button>
</form>"#,
        target = url_path(target),
        store = url_path(store),
        csrf = attr(csrf),
        resource_attr = attr(resource_id),
    )
}

fn render_libsecret_write_form(
    target: &str,
    store: &str,
    resource_id: &str,
    csrf: &str,
) -> String {
    format!(
        r#"<form method="post" action="/t/{target}/s/{store}/secret/write-libsecret" class="card">
  <input type="hidden" name="csrf_token" value="{csrf}"/>
  <input type="hidden" name="resource_id" value="{resource_attr}"/>
  <h3>Set libsecret slot entry</h3>
  <div class="field"><label>item id</label><input name="item_id" required/></div>
  <div class="field"><label>serialized value</label><textarea name="value" rows="6" required></textarea></div>
  <button type="submit">Write libsecret entry</button>
</form>"#,
        target = url_path(target),
        store = url_path(store),
        csrf = attr(csrf),
        resource_attr = attr(resource_id),
    )
}

fn render_access_matrix(
    target: &str,
    store: &str,
    policy: &PolicyDocument,
    catalog: &[CatalogEntry],
    selected_resource: Option<&str>,
    csrf: &str,
) -> String {
    let Some(selected_resource) = selected_resource else {
        return r#"<p class="muted">Select a resource to edit access.</p>"#.to_string();
    };

    let rows: Vec<&CatalogEntry> = catalog
        .iter()
        .filter(|entry| entry.resource == selected_resource)
        .collect();
    if rows.is_empty() {
        return r#"<p class="muted">No frontend has registered this resource yet.</p>"#.to_string();
    }

    let body_rows: Vec<String> = rows
        .iter()
        .map(|entry| {
            let cells: String = OPERATIONS
                .iter()
                .map(|operation| render_matrix_cell(target, store, policy, entry, operation, csrf))
                .collect();
            format!(
                r#"<tr>
  <td class="mono">{host}</td>
  <td class="mono">{user}</td>
  <td class="mono">{wrapper}</td>
  {cells}
</tr>"#,
                host = h(&entry.host),
                user = h(&entry.user),
                wrapper = h(&entry.wrapper),
                cells = cells,
            )
        })
        .collect();

    let headers: String = OPERATIONS
        .iter()
        .map(|op| format!("<th class=\"mono\">{}</th>", h(op)))
        .collect();

    format!(
        r#"<p class="muted">Rows are auto-registered when a frontend daemon starts. Toggle permanent allow rules for the selected resource.</p>
<table>
<thead><tr><th>host</th><th>user</th><th>wrapper</th>{headers}</tr></thead>
<tbody>
{rows}
</tbody>
</table>"#,
        headers = headers,
        rows = body_rows.join("\n"),
    )
}

fn render_matrix_cell(
    target: &str,
    store: &str,
    policy: &PolicyDocument,
    entry: &CatalogEntry,
    operation: &str,
    csrf: &str,
) -> String {
    let enabled = policy.rules.iter().any(|rule| {
        rule.wrapper == entry.wrapper
            && rule.resource == format!("{}:{}", operation.split('.').next().unwrap_or("env"), entry.resource)
            && rule.operation == operation
            && rule.effect == PolicyEffect::Allow
    });
    let resource_kind = operation.split('.').next().unwrap_or("env");
    let resource = format!("{resource_kind}:{}", entry.resource);
    format!(
        r#"<td>
  <form method="post" action="/t/{target}/s/{store}/matrix/toggle" class="inline-form">
    <input type="hidden" name="csrf_token" value="{csrf}"/>
    <input type="hidden" name="wrapper" value="{wrapper}"/>
    <input type="hidden" name="resource" value="{resource_attr}"/>
    <input type="hidden" name="operation" value="{operation_attr}"/>
    <input type="hidden" name="enabled" value="{enabled_value}"/>
    <button type="submit">{label}</button>
  </form>
</td>"#,
        target = url_path(target),
        store = url_path(store),
        csrf = attr(csrf),
        wrapper = attr(&entry.wrapper),
        resource_attr = attr(&resource),
        operation_attr = attr(operation),
        enabled_value = if enabled { "off" } else { "on" },
        label = if enabled { "✓" } else { "·" },
    )
}

fn render_wrappers(target: &str, store: &str, policy: &PolicyDocument, csrf: &str) -> String {
    if policy.wrappers.is_empty() {
        return r#"<p class="muted">No wrapper states yet.</p>"#.to_string();
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

#[cfg(test)]
mod tests {
    use super::*;
    use underwrap_protocol::policy::{PolicyRule, WrapperPolicyState};

    fn sample_policy() -> PolicyDocument {
        PolicyDocument {
            wrappers: vec![WrapperPolicyState {
                wrapper: "robert-gh-authorized".to_string(),
                mode: WrapperApprovalMode::Unlocked { until_epoch: None },
                reason: None,
            }],
            rules: vec![PolicyRule {
                id: "r1".to_string(),
                wrapper: "robert-gh-authorized".to_string(),
                resource: "env:gh/main".to_string(),
                operation: "env.read".to_string(),
                effect: PolicyEffect::Allow,
            }],
            grants: vec![],
        }
    }

    #[test]
    fn instance_page_renders_resource_selector_and_matrix() {
        let policy = sample_policy();
        let catalog = vec![
            CatalogEntry {
                host: "deepthought".to_string(),
                user: "robert".to_string(),
                wrapper: "robert-gh-authorized".to_string(),
                resource: "gh/main".to_string(),
            },
            CatalogEntry {
                host: "smallthought".to_string(),
                user: "robert".to_string(),
                wrapper: "robert-gh-authorized".to_string(),
                resource: "gh/main".to_string(),
            },
        ];
        let secret_ids = vec![
            "gh/main/files/hosts.yml".to_string(),
            "gh/main/libsecret/default/item_1".to_string(),
        ];
        let html = instance_page(InstancePageData {
            operator: "alice",
            target: "corp",
            store: "work",
            stores: &["work".to_string()],
            policy: &policy,
            catalog: &catalog,
            secret_ids: &secret_ids,
            selected_resource: Some("gh/main"),
            csrf_token: "CSRF123",
            banner_ok: Some("saved"),
            banner_error: None,
        });
        assert!(html.contains("Access matrix"));
        assert!(html.contains("deepthought"));
        assert!(html.contains("smallthought"));
        assert!(html.contains("env.read"));
        assert!(html.contains("/matrix/toggle"));
        assert!(html.contains("hosts.yml"));
        assert!(html.contains("item_1"));
    }

    #[test]
    fn instance_page_escapes_policy_content() {
        let policy = PolicyDocument {
            wrappers: vec![WrapperPolicyState {
                wrapper: "<wrap>".to_string(),
                mode: WrapperApprovalMode::Locked,
                reason: Some("<reason>".to_string()),
            }],
            rules: vec![],
            grants: vec![],
        };
        let html = instance_page(InstancePageData {
            operator: "alice",
            target: "corp",
            store: "work",
            stores: &["work".to_string()],
            policy: &policy,
            catalog: &[],
            secret_ids: &[],
            selected_resource: None,
            csrf_token: "CSRF",
            banner_ok: None,
            banner_error: None,
        });
        assert!(html.contains("&lt;wrap&gt;"));
        assert!(html.contains("&lt;reason&gt;"));
    }
}
