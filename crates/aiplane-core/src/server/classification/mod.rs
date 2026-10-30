// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Data classification: the operator's ordered levels, the data classes that
//! map onto them, the maximum level of every destination, and the level a
//! conversation has reached.
//!
//! This module holds types and database accessors only. Detecting data
//! (System One), hiding models and refusing turns live in the layers above.

mod conversation;
mod policy;
mod scheme;
mod store;

pub use conversation::{
    ClassificationError, Transition, level_id, lower, raise, record_event, set_level,
};
pub use policy::{ClassificationConfig, ClassificationMode, GateOnMatch, Lowering, OutputMarking};
pub use scheme::{Action, Assignment, DataClass, DestinationKind, Level, Over, Rule, Scheme};
pub use store::{assignments, load_scheme, pool_ranks};
