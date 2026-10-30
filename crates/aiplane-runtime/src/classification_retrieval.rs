//! Retrieval from a knowledge collection carries the collection's level into
//! the conversation: what a collection returns is as sensitive as the
//! collection, so the conversation rises before the model reads it.

use aiplane_core::server::classification::{self, ClassificationMode, DestinationKind};

use crate::server::tools::ToolContext;

/// The cause audited when a retrieval raises a conversation.
pub const RETRIEVAL_CAUSE: &str = "retrieval";

/// Raise the conversation to the level of collection `collection_id`. A
/// collection without an assignment is the lowest level and raises nothing.
/// Monitor mode, agent runs and calls outside a conversation never raise.
///
/// An error means the level could not be stored; the caller must not hand the
/// retrieved text to the model then.
pub async fn raise_for_collection(ctx: &ToolContext, collection_id: i64) -> Result<(), String> {
    let (Some(policy), Some(session_id)) = (
        ctx.classification
            .as_ref()
            .filter(|policy| policy.scheme.is_active()),
        ctx.session_id.as_deref(),
    ) else {
        return Ok(());
    };
    if ctx.agent.is_some() || policy.config.mode == ClassificationMode::Monitor {
        return Ok(());
    }
    let Some(assignment) = policy
        .scheme
        .assignment(DestinationKind::Collection, &collection_id.to_string())
    else {
        return Ok(());
    };
    classification::raise(
        &ctx.db,
        &policy.scheme,
        session_id,
        &assignment.level_id,
        RETRIEVAL_CAUSE,
        ctx.principal.subject_id(),
    )
    .await
    .map(|_| ())
    .map_err(|error| {
        format!(
            "could not record the conversation level after reading a classified collection: {error}. \
             The results were withheld; retry the call."
        )
    })
}
