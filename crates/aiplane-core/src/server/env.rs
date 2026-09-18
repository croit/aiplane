// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Environment variables, read under two names.
//!
//! The product is croit AIplane; it used to be called croit LLM Gateway, and
//! every variable it reads was spelled `GATEWAY_*`. Renaming them outright
//! would stop a working deployment dead at boot — `GATEWAY_SESSION_KEY` seals
//! every secret in the database, so "the new binary ignored it" is not a
//! cosmetic failure, it is an unreadable install.
//!
//! So each variable now has two spellings: `AIPLANE_*` is canonical and wins,
//! `GATEWAY_*` still works and logs one deprecation warning per variable per
//! process. The legacy names are removed only in a deliberate breaking
//! release. See `docs/renaming.md`.
//!
//! Every read of a gateway-owned variable goes through [`var`] / [`var_os`],
//! so a new variable gets both spellings for free and no call site has to
//! remember the rule.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::sync::Mutex;

/// Canonical prefix. Everything the process owns is spelled with this.
pub const PREFIX: &str = "AIPLANE_";

/// The pre-rename prefix, still honoured. Deprecated.
pub const LEGACY_PREFIX: &str = "GATEWAY_";

/// Variables we have already warned about, so a value read on every request
/// (or in a loop) does not produce a log line each time.
static WARNED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// The legacy spelling of a canonical `AIPLANE_*` name, if there is one.
///
/// Anything not starting with [`PREFIX`] is passed through unchanged: names
/// like `RUST_LOG` or `SEARCH_PROVIDER` are not ours to rename.
fn legacy_name(name: &str) -> Option<String> {
    name.strip_prefix(PREFIX)
        .map(|rest| format!("{LEGACY_PREFIX}{rest}"))
}

fn warn_once(legacy: &str, canonical: &str) {
    let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
    if warned.insert(legacy.to_string()) {
        tracing::warn!(
            "${legacy} is the old name for ${canonical} and still works, but it is deprecated \
             and will be removed in a future release — rename it (docs/renaming.md)"
        );
    }
}

/// Read `name`, falling back to its `GATEWAY_*` spelling.
///
/// Pass the canonical `AIPLANE_*` name; the fallback is derived. A value set
/// under the canonical name wins outright, so a deployment can set both
/// during a migration without ambiguity.
pub fn var(name: &str) -> Result<String, std::env::VarError> {
    match std::env::var(name) {
        Ok(v) => Ok(v),
        Err(e) => match legacy_name(name) {
            Some(legacy) => match std::env::var(&legacy) {
                Ok(v) => {
                    warn_once(&legacy, name);
                    Ok(v)
                }
                Err(_) => Err(e),
            },
            None => Err(e),
        },
    }
}

/// [`var`] for values that are paths and need not be UTF-8.
pub fn var_os(name: &str) -> Option<OsString> {
    if let Some(v) = std::env::var_os(name) {
        return Some(v);
    }
    let legacy = legacy_name(name)?;
    let v = std::env::var_os(&legacy)?;
    warn_once(&legacy, name);
    Some(v)
}

/// Whether `name` is set under either spelling.
pub fn is_set(name: &str) -> bool {
    var_os(name).is_some()
}

/// Both spellings of `name`, canonical first — for error messages that have
/// to tell an operator what to set.
pub fn both_names(name: &str) -> (String, Option<String>) {
    (name.to_string(), legacy_name(name))
}

/// Variables the container image bakes a default for. A legacy value set on
/// such a variable would lose to the image's own canonical default and be
/// silently ignored — and for a data directory that looks exactly like an
/// empty database. These are checked at boot rather than left to chance.
pub const IMAGE_BAKED: &[&str] = &["AIPLANE_DATA_DIR", "AIPLANE_STATIC_DIR"];

/// Refuse to start when a variable is set under both spellings with different
/// values.
///
/// Canonical wins, which is the right rule everywhere except against a
/// default the image itself put in the environment: there the operator's
/// `GATEWAY_*` value is the intent and the canonical value is boilerplate.
/// Rather than guess which is which, say so and stop — a boot that fails with
/// both values printed costs a minute; picking the wrong one costs a database.
pub fn check_conflicts(names: &[&str]) -> Result<(), String> {
    for name in names {
        let Some(legacy) = legacy_name(name) else {
            continue;
        };
        let (Some(new), Some(old)) = (std::env::var_os(name), std::env::var_os(&legacy)) else {
            continue;
        };
        if new != old {
            return Err(format!(
                "${name} and ${legacy} are both set to different values ({new:?} vs {old:?}). \
                 ${legacy} is the deprecated spelling and the two cannot both be honoured — \
                 unset one of them (docs/renaming.md)"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the tests: they mutate process-wide environment.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn canonical_name_wins_over_legacy() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("AIPLANE_TEST_PRECEDENCE", "new");
            std::env::set_var("GATEWAY_TEST_PRECEDENCE", "old");
        }
        assert_eq!(var("AIPLANE_TEST_PRECEDENCE").unwrap(), "new");
        unsafe {
            std::env::remove_var("AIPLANE_TEST_PRECEDENCE");
            std::env::remove_var("GATEWAY_TEST_PRECEDENCE");
        }
    }

    #[test]
    fn legacy_name_is_still_read() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("GATEWAY_TEST_FALLBACK", "old") };
        assert_eq!(var("AIPLANE_TEST_FALLBACK").unwrap(), "old");
        assert_eq!(var_os("AIPLANE_TEST_FALLBACK").unwrap(), "old");
        assert!(is_set("AIPLANE_TEST_FALLBACK"));
        unsafe { std::env::remove_var("GATEWAY_TEST_FALLBACK") };
    }

    #[test]
    fn unset_under_both_names_is_unset() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert!(var("AIPLANE_TEST_DEFINITELY_UNSET").is_err());
        assert!(var_os("AIPLANE_TEST_DEFINITELY_UNSET").is_none());
        assert!(!is_set("AIPLANE_TEST_DEFINITELY_UNSET"));
    }

    #[test]
    fn foreign_names_get_no_fallback() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(legacy_name("RUST_LOG"), None);
        assert_eq!(
            legacy_name("AIPLANE_SESSION_KEY").as_deref(),
            Some("GATEWAY_SESSION_KEY")
        );
    }

    #[test]
    fn conflicting_spellings_are_refused() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("AIPLANE_TEST_CONFLICT", "/from/image");
            std::env::set_var("GATEWAY_TEST_CONFLICT", "/from/operator");
        }
        let err = check_conflicts(&["AIPLANE_TEST_CONFLICT"]).unwrap_err();
        assert!(err.contains("GATEWAY_TEST_CONFLICT"), "{err}");
        // Same value under both names is how the Helm chart writes the
        // session key; it must not be treated as a conflict.
        unsafe { std::env::set_var("GATEWAY_TEST_CONFLICT", "/from/image") };
        assert!(check_conflicts(&["AIPLANE_TEST_CONFLICT"]).is_ok());
        unsafe {
            std::env::remove_var("AIPLANE_TEST_CONFLICT");
            std::env::remove_var("GATEWAY_TEST_CONFLICT");
        }
        assert!(check_conflicts(&["AIPLANE_TEST_CONFLICT"]).is_ok());
    }

    #[test]
    fn both_names_reports_the_pair() {
        let (canonical, legacy) = both_names("AIPLANE_DB_PATH");
        assert_eq!(canonical, "AIPLANE_DB_PATH");
        assert_eq!(legacy.as_deref(), Some("GATEWAY_DB_PATH"));
    }
}
