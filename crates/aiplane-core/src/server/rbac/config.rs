// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

#[derive(Debug, Clone, Default)]
pub struct RbacConfig {
    /// Role applied to every authenticated user before mapping. Use this for
    /// a baseline "logged-in user" tier. Optional — when unset, users with
    /// no matching mapping get no role IDs.
    pub default_role: Option<String>,
    /// Maps an OIDC claim value to an internal role ID. Multiple mappings can
    /// point at the same role.
    pub mappings: Vec<RoleMapping>,
}

#[derive(Debug, Clone)]
pub struct RoleMapping {
    pub oidc_value: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleConfig {
    pub id: String,
    /// When true, holding this role grants access to the admin UI (`/admin/*`)
    /// and admin-only actions. Replaces the former hardcoded "role literally
    /// named `admin`" check — name roles freely and flag whichever ones are
    /// privileged; more than one may carry it. Defaults to false, so adding a
    /// role never silently grants admin.
    pub admin: bool,
    /// Tool IDs this role grants, matched exactly. The one non-id value is
    /// `"*"`, which expands to every registered tool at resolve time (and also
    /// unlocks the ComfyUI workflows, which are in no registry). Globs are not
    /// supported: `"some*"` is looked up as that literal id and grants nothing.
    pub tools: Vec<String>,
    /// Skill names this role grants. `"*"` expands to every loaded skill.
    /// Empty (the default) means the role sees no skills, so a deployment
    /// that enables skills doesn't silently expose them to every role.
    /// Gates both the system-message skill listing and the `read_skill`
    /// tool. See `server::skills`.
    pub skills: Vec<String>,
}
