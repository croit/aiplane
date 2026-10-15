// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The agent builder's persistence and the pieces only agents need
//! (`docs/agents.md`): the accessors of its tables ([`db`]), the visitor rate
//! gate ([`rates`]) and the inbox's chat webhooks ([`notify_channels`]).
//!
//! Sits on `aiplane-core` beside `aiplane-features`, below `aiplane-runtime`.
//! Nothing here names `AppState`, the tool registry or a feature subsystem,
//! and nothing below it names an agent. See `docs/architecture.md`.

pub mod db;
pub mod notify_channels;
pub mod rates;

/// The suffix of a JSON key whose string value is sealed (`"<nonce>.<ct>"`,
/// `Crypto::seal_to_string`). Agent specs keep their credentials this way
/// (`token_sealed`, `secret_sealed`, …), so a credential is found by its key
/// without knowing the spec's layout.
pub const SEALED_SUFFIX: &str = "_sealed";
