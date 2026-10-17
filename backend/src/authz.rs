//! Scoped role-based access control.
//!
//! A membership grants a role at one node of the organisation tree. The role's
//! permissions apply to that node and every node beneath it, so a headmaster
//! assigned at the school root can act everywhere, while a teacher assigned to
//! "Grade 4 / Section A" can only act inside that section.

use std::collections::HashSet;

use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

pub struct PermissionDef {
    pub key: &'static str,
    pub group: &'static str,
    pub label: &'static str,
}

macro_rules! perms {
    ($($key:literal, $group:literal, $label:literal;)*) => {
        pub const CATALOG: &[PermissionDef] = &[
            $(PermissionDef { key: $key, group: $group, label: $label },)*
        ];
    };
}

perms! {
    "admin.users.manage",      "Administration", "Create users, assign roles, lock and reset accounts";
    "admin.roles.manage",      "Administration", "Create roles and edit their permissions";
    "admin.org.manage",        "Administration", "Edit the organisation tree";
    "admin.workflows.manage",  "Administration", "Design and publish approval workflows";
    "admin.forms.manage",      "Administration", "Build forms and attach workflows";
    "admin.settings.manage",   "Administration", "Change modules, branding and security policy";
    "admin.audit.view",        "Administration", "Read and verify the audit trail";
    "admin.sessions.manage",   "Administration", "View and revoke other users' sessions";
    "people.view",             "People",         "See the directory of people in scope";
    "content.view",            "Content",        "Read published announcements and pages";
    "content.create",          "Content",        "Draft announcements and pages";
    "content.publish",         "Content",        "Publish content without an approval round";
    "social.view",             "Community",      "Read the community feed";
    "social.post",             "Community",      "Write posts";
    "social.comment",          "Community",      "Comment and react";
    "social.post_direct",      "Community",      "Post without moderation";
    "social.moderate",         "Community",      "Hide posts and comments";
    "approvals.view_all",      "Approvals",      "See every approval request in scope";
    "forms.submit",            "Forms",          "Submit forms";
}

pub fn is_known(perm: &str) -> bool {
    CATALOG.iter().any(|p| p.key == perm)
}

#[derive(Debug, Clone, Serialize)]
pub struct Grant {
    pub membership_id: Uuid,
    pub org_unit_id: Uuid,
    pub org_unit_name: String,
    pub path: String,
    pub role_key: String,
    pub role_name: String,
    #[serde(skip)]
    pub perms: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct Principal {
    pub user_id: Uuid,
    pub grants: Vec<Grant>,
}

/// `a` is an ancestor of (or equal to) `b` in ltree label space.
pub fn path_covers(ancestor: &str, descendant: &str) -> bool {
    descendant == ancestor
        || (descendant.len() > ancestor.len()
            && descendant.starts_with(ancestor)
            && descendant.as_bytes()[ancestor.len()] == b'.')
}

impl Principal {
    pub async fn load(db: &PgPool, user_id: Uuid) -> AppResult<Self> {
        let rows: Vec<(Uuid, Uuid, String, String, String, String, Option<Vec<String>>)> = sqlx::query_as(
            r#"select m.id, o.id, o.name, o.path::text, r.key, r.name,
                      array_remove(array_agg(rp.permission), null)
               from memberships m
               join org_units o on o.id = m.org_unit_id
               join roles r on r.id = m.role_id
               left join role_permissions rp on rp.role_id = r.id
               where m.user_id = $1
               group by m.id, o.id, r.id
               order by nlevel(o.path), r.name"#,
        )
        .bind(user_id)
        .fetch_all(db)
        .await?;
        let grants = rows
            .into_iter()
            .map(|(membership_id, org_unit_id, org_unit_name, path, role_key, role_name, perms)| Grant {
                membership_id,
                org_unit_id,
                org_unit_name,
                path,
                role_key,
                role_name,
                perms: perms.unwrap_or_default().into_iter().collect(),
            })
            .collect();
        Ok(Self { user_id, grants })
    }

    /// Holds `perm` at `path` (via a grant at that node or any ancestor).
    pub fn can_at(&self, perm: &str, path: &str) -> bool {
        self.grants.iter().any(|g| g.perms.contains(perm) && path_covers(&g.path, path))
    }

    /// Holds `perm` somewhere in the tree.
    pub fn can_anywhere(&self, perm: &str) -> bool {
        self.grants.iter().any(|g| g.perms.contains(perm))
    }

    pub fn require_at(&self, perm: &str, path: &str) -> AppResult<()> {
        if self.can_at(perm, path) { Ok(()) } else { Err(AppError::Forbidden) }
    }

    pub fn require_anywhere(&self, perm: &str) -> AppResult<()> {
        if self.can_anywhere(perm) { Ok(()) } else { Err(AppError::Forbidden) }
    }

    /// Roots of every subtree in which the principal holds `perm`; feed these to
    /// SQL as `path <@ any($n::ltree[])`.
    pub fn scope_paths(&self, perm: &str) -> Vec<String> {
        let mut roots: Vec<String> = self
            .grants
            .iter()
            .filter(|g| g.perms.contains(perm))
            .map(|g| g.path.clone())
            .collect();
        roots.sort_by_key(|p| p.len());
        let mut out: Vec<String> = Vec::new();
        for p in roots {
            if !out.iter().any(|o| path_covers(o, &p)) {
                out.push(p);
            }
        }
        out
    }

    /// Every path where the user holds any membership at all.
    pub fn member_paths(&self) -> Vec<String> {
        self.grants.iter().map(|g| g.path.clone()).collect()
    }

    pub fn role_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.grants.iter().map(|g| g.role_key.clone()).collect();
        keys.sort();
        keys.dedup();
        keys
    }

    pub fn all_permissions(&self) -> Vec<String> {
        let mut set: Vec<String> = self.grants.iter().flat_map(|g| g.perms.iter().cloned()).collect();
        set.sort();
        set.dedup();
        set
    }

    /// The deepest node the user belongs to; the default "home" unit for
    /// requests and posts.
    pub fn home_unit(&self) -> Option<&Grant> {
        self.grants.iter().max_by_key(|g| g.path.matches('.').count())
    }
}

pub async fn unit_path(db: &PgPool, id: Uuid) -> AppResult<String> {
    sqlx::query_scalar::<_, String>("select path::text from org_units where id = $1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers() {
        assert!(path_covers("a", "a"));
        assert!(path_covers("a", "a.b"));
        assert!(!path_covers("a", "ab"));
        assert!(!path_covers("a.b", "a"));
    }
}
