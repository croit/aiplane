// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Credentials inside a spec: an A2A route's token or client secret and a
//! `host_jwt` verifier's HMAC secret. Each is sealed once the spec is valid,
//! before it is stored or echoed, so no draft, revision, version, audit row
//! or GET carries it in clear.
//!
//! A sealed credential is kept as `<key>` + [`SEALED_SUFFIX`] beside where
//! the plaintext was.

use aiplane_agents::SEALED_SUFFIX;
use aiplane_core::server::crypto::Crypto;
use serde_json::Value;

/// Where a spec keeps a credential.
pub struct SecretSlot {
    /// The spec's top-level map whose entries hold it.
    pub collection: &'static str,
    /// JSON pointer, inside one entry, to the object holding the keys; `""`
    /// for the entry itself.
    pub at: &'static str,
    /// Only entries of this `kind`.
    pub kind: Option<&'static str>,
    pub keys: &'static [&'static str],
    /// What a sealing failure names.
    pub what: &'static str,
}

/// An `a2a` route's bearer token or OAuth client secret.
pub const A2A_AUTH: SecretSlot = SecretSlot {
    collection: "routes",
    at: "/a2a/auth",
    kind: None,
    keys: &["token", "client_secret"],
    what: "the A2A credential",
};

/// A `host_jwt` verifier's HS256 secret.
pub const HOST_JWT_SECRET: SecretSlot = SecretSlot {
    collection: "verifiers",
    at: "",
    kind: Some("host_jwt"),
    keys: &["secret"],
    what: "the host_jwt secret",
};

/// Every credential a spec can carry.
pub const SPEC_SECRETS: &[SecretSlot] = &[A2A_AUTH, HOST_JWT_SECRET];

/// Replace every plaintext credential `slots` name with its sealed form. A
/// credential sealed already stays as it is.
pub fn seal_spec_secrets(
    spec: &mut Value,
    slots: &[SecretSlot],
    crypto: &Crypto,
) -> Result<(), String> {
    for slot in slots {
        let Some(entries) = spec.get_mut(slot.collection).and_then(Value::as_object_mut) else {
            continue;
        };
        for entry in entries.values_mut() {
            if slot
                .kind
                .is_some_and(|kind| entry.get("kind").and_then(Value::as_str) != Some(kind))
            {
                continue;
            }
            let Some(holder) = entry.pointer_mut(slot.at).and_then(Value::as_object_mut) else {
                continue;
            };
            for key in slot.keys {
                if let Some(Value::String(secret)) = holder.remove(*key) {
                    let sealed = crypto
                        .seal_to_string(&secret)
                        .map_err(|e| format!("sealing {} failed: {e}", slot.what))?;
                    holder.insert(format!("{key}{SEALED_SUFFIX}"), Value::String(sealed));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_slot_seals_in_place_and_leaves_everything_else() {
        let crypto = Crypto::ephemeral();
        let mut spec = json!({
            "routes": {
                "partner": { "a2a": { "auth": { "kind": "bearer", "token": "t-1" } } },
                "oauth": { "a2a": { "auth": { "client_id": "gw", "client_secret": "c-2" } } },
                "local": { "agent": "billing" }
            },
            "verifiers": {
                "site": { "kind": "host_jwt", "secret": "s-3" },
                "otp": { "kind": "mcp_code", "secret": "left alone" }
            }
        });
        seal_spec_secrets(&mut spec, SPEC_SECRETS, &crypto).unwrap();
        let open = |v: &Value| crypto.open_from_string(v.as_str().unwrap());
        let partner = &spec["routes"]["partner"]["a2a"]["auth"];
        assert_eq!(open(&partner["token_sealed"]).as_deref(), Some("t-1"));
        assert!(partner.get("token").is_none());
        let oauth = &spec["routes"]["oauth"]["a2a"]["auth"];
        assert_eq!(open(&oauth["client_secret_sealed"]).as_deref(), Some("c-2"));
        assert_eq!(oauth["client_id"], "gw");
        let site = &spec["verifiers"]["site"];
        assert_eq!(open(&site["secret_sealed"]).as_deref(), Some("s-3"));
        assert_eq!(spec["verifiers"]["otp"]["secret"], "left alone");

        let again = spec.clone();
        seal_spec_secrets(&mut spec, SPEC_SECRETS, &crypto).unwrap();
        assert_eq!(spec, again, "a sealed credential stays as it is");
    }
}
