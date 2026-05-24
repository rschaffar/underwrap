//! Structured `admin event` log emitter.
//!
//! Every admin mutation calls [`log_admin_event`] after the action
//! completes (success or failure) so operators have a single grep
//! target for audit-relevant lines. The shape of the `tracing::info!`
//! line is part of the audit contract: field names and which
//! `Option<_>` slots are `Some`/`None` per entity kind are asserted by
//! `audit/tests.rs`.
//!
//! Per-subdomain wrappers used to format the entity string by hand;
//! they have all been replaced by the [`AuditEntity`] enum so the
//! `kind:id` formatting lives in exactly one place.

use tracing::info;

use underwrap_protocol::policy::WrapperPermission;
use underwrap_protocol::resource::ResourceSlot;

use crate::users::Principal;

/// Identifies what an admin event acted on. Each variant maps to a
/// distinct `kind:` prefix in the formatted `resource` field, and
/// may also surface a `wrapper:` field where the entity carries that
/// information (pairs, permissions).
pub(crate) enum AuditEntity<'a> {
    User {
        username: &'a str,
    },
    Store {
        store: &'a str,
    },
    ClientInstance {
        id: &'a str,
    },
    Resource {
        resource_id: &'a str,
    },
    Pair {
        wrapper: &'a str,
        resource_id: &'a str,
    },
    Permission {
        store: &'a str,
        permission: &'a WrapperPermission,
    },
    Secret {
        store: &'a str,
        resource_id: &'a str,
        slot: ResourceSlot,
        key: &'a str,
    },
    /// Structured libsecret entity inside a resource. The `kind`
    /// discriminator (`alias` / `collection` / `item`) carries the
    /// spec concept so operators can filter audit lines per Secret
    /// Service object without parsing the flat-key shape.
    LibsecretEntity {
        store: &'a str,
        resource_id: &'a str,
        kind: LibsecretEntityKind,
        /// Alias name, collection id, or `<collection>/<item>`
        /// depending on `kind`.
        path: &'a str,
    },
}

/// The Secret Service object kind an `AuditEntity::LibsecretEntity`
/// refers to. Distinct from `ResourceSlot::Libsecret`, which only
/// disambiguates the top-level slot inside a resource.
#[derive(Copy, Clone)]
pub(crate) enum LibsecretEntityKind {
    Alias,
    Collection,
    Item,
}

impl LibsecretEntityKind {
    fn name(self) -> &'static str {
        match self {
            LibsecretEntityKind::Alias => "alias",
            LibsecretEntityKind::Collection => "collection",
            LibsecretEntityKind::Item => "item",
        }
    }
}

impl AuditEntity<'_> {
    /// The `resource:` field as logged. Operators grep these prefixes,
    /// so the shape is load-bearing.
    fn resource_field(&self) -> String {
        match self {
            AuditEntity::User { username } => format!("user:{username}"),
            AuditEntity::Store { store } => format!("store:{store}"),
            AuditEntity::ClientInstance { id } => format!("client-instance:{id}"),
            AuditEntity::Resource { resource_id } => format!("resource:{resource_id}"),
            AuditEntity::Pair {
                wrapper,
                resource_id,
            } => format!("pair:{wrapper}/{resource_id}"),
            AuditEntity::Permission { store, permission } => format!(
                "wrapper_permission:{store}/{}/{}/{}",
                permission.client_instance, permission.wrapper, permission.resource_id,
            ),
            AuditEntity::Secret {
                store,
                resource_id,
                slot,
                key,
            } => format!("secret:{store}/{resource_id}/{}/{key}", slot.name()),
            AuditEntity::LibsecretEntity {
                store,
                resource_id,
                kind,
                path,
            } => format!("libsecret-{}:{store}/{resource_id}/{path}", kind.name()),
        }
    }

    /// The `wrapper:` field as logged. Surfaces only on entities that
    /// inherently bind a wrapper name (pairs, permissions); everything
    /// else logs `None`.
    fn wrapper_field(&self) -> Option<&str> {
        match self {
            AuditEntity::Pair { wrapper, .. } => Some(wrapper),
            AuditEntity::Permission { permission, .. } => Some(permission.wrapper.as_str()),
            AuditEntity::User { .. }
            | AuditEntity::Store { .. }
            | AuditEntity::ClientInstance { .. }
            | AuditEntity::Resource { .. }
            | AuditEntity::Secret { .. }
            | AuditEntity::LibsecretEntity { .. } => None,
        }
    }
}

/// Emit one admin audit line. Every admin mutation calls this after
/// the action completes; the `result` is either `"success"` or
/// `"failure"` (the latter typically paired with a non-`None`
/// `operation` describing the failure reason).
pub(crate) fn log_admin_event(
    action: &str,
    operator: &Principal,
    entity: AuditEntity<'_>,
    operation: Option<&str>,
    result: &str,
) {
    let resource = entity.resource_field();
    emit(
        action,
        &operator.username,
        entity.wrapper_field(),
        Some(resource.as_str()),
        None,
        operation,
        result,
    );
}

/// Emit a bootstrap-attempt audit line. Distinct from
/// [`log_admin_event`] because at bootstrap time no [`Principal`]
/// record exists yet — the username the caller is *trying* to create
/// is what we log as the operator. No entity record either; the
/// would-be user shows up in the `operator` field.
pub(crate) fn log_bootstrap_attempt(username: &str, operation: Option<&str>, result: &str) {
    emit(
        "bootstrap.attempt",
        username,
        None,
        None,
        None,
        operation,
        result,
    );
}

fn emit(
    action: &str,
    operator: &str,
    wrapper: Option<&str>,
    resource: Option<&str>,
    secret_id: Option<&str>,
    operation: Option<&str>,
    result: &str,
) {
    info!(
        action = %action,
        operator = %operator,
        wrapper = ?wrapper,
        resource = ?resource,
        secret_id = ?secret_id,
        operation = ?operation,
        result = %result,
        "admin event"
    );
}

#[cfg(test)]
mod tests;
