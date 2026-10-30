//! The level a person's new conversation starts at.

use aiplane_core::server::classification;
use aiplane_core::server::db::DbError;

use crate::server::AppState;

/// Give a conversation `user_id` just created the policy's default level.
/// Materialised here, not resolved on read, so changing the default later does
/// not move conversations that already exist. A default at the lowest level is
/// the absence of a stored level.
pub async fn apply_default_level(
    state: &AppState,
    session_id: &str,
    user_id: &str,
) -> Result<(), DbError> {
    let scheme = state.scheme();
    if !scheme.is_active() {
        return Ok(());
    }
    let level = scheme.resolve(&state.config().classification.default_level);
    if level.rank == 0 {
        return Ok(());
    }
    classification::set_level(&state.db, session_id, &level.id).await?;
    classification::record_event(
        &state.db,
        session_id,
        &scheme.lowest().id,
        &level.id,
        "default",
        user_id,
    )
    .await
}
