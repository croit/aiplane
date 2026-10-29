// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::personal_workspace_scope;
use crate::persona::personal_dot::{PersonalDotContext, PersonalTurnContext, initialize_for_turn};
use crate::server::tools::ToolContext;
use aiplane_agents::db::{agents, dot_sessions, system_principals::NewPrincipal};
use aiplane_core::server::db::users::{self, User};
use aiplane_core::server::principal::Principal;
use session_core::db::{self as chat, TurnStatus};
use shared::sandbox::{DotWorkspace, DotWorkspaceScope, WorkspacePurpose};

async fn running_context() -> ToolContext {
    let state = crate::server::headless::tests::state_for("http://127.0.0.1:1").await;
    let now = jiff::Timestamp::now();
    let user = User {
        id: "workspace-owner".into(),
        email: "workspace-owner@example.com".into(),
        name: None,
        roles: vec![],
        created_at: now,
        updated_at: now,
        timezone: None,
        speech_voice: None,
    };
    users::upsert(&state.db, &user).await.unwrap();
    let (_, dot) = agents::create_personal_dot(
        &state.db,
        &NewPrincipal {
            name: "Harald",
            display: "Harald",
            description: "",
        },
        r#"{"main":{"tools":[]}}"#,
        &user.id,
        4,
    )
    .await
    .unwrap();
    let dot = dot.unwrap();
    let session = chat::create_session(&state.db, &user.id).await.unwrap();
    assert!(
        dot_sessions::link(&state.db, &user.id, &dot.principal.id, &session.id)
            .await
            .unwrap()
    );
    let turn = chat::create_assistant_turn_in_progress(
        &state.db,
        &session.id,
        "workspace-assistant",
        "model-a",
    )
    .await
    .unwrap();
    let Some(PersonalDotContext::Active { actor, .. }) = initialize_for_turn(
        &state,
        &user,
        true,
        PersonalTurnContext {
            session_id: &session.id,
            assistant_turn_id: &turn.id,
            model: "model-a",
            clock: crate::budget::system_clock(),
            resume_checkpoint: None,
            child_execution: None,
        },
    )
    .await
    .unwrap() else {
        panic!("The owned running agent fixture must initialize its execution");
    };
    ToolContext {
        principal: actor.principal(),
        personal_dot: actor.personal_dot_identity().cloned(),
        personal_execution: actor.personal_execution().cloned(),
        session_id: Some(session.id),
        assistant_turn_id: Some(turn.id),
        ..ToolContext::for_test(state.db.clone())
    }
}

#[tokio::test]
async fn workspace_scope_uses_the_verified_owner_dot_turn_and_requested_purpose() {
    let context = running_context().await;
    let identity = context.personal_dot.as_ref().unwrap();
    for purpose in [WorkspacePurpose::Workspace, WorkspacePurpose::Browser] {
        assert_eq!(
            personal_workspace_scope(&context, purpose).await.unwrap(),
            Some(DotWorkspaceScope {
                workspace: DotWorkspace {
                    owner_user_id: identity.owner_user_id.clone(),
                    dot_id: identity.dot_id.clone(),
                },
                turn_id: context.assistant_turn_id.clone().unwrap(),
                purpose,
            })
        );
    }
}

#[tokio::test]
async fn workspace_scope_rejects_mismatched_context_identity_session_and_turn() {
    let mut context = running_context().await;
    let original_principal = context.principal.clone();
    context.principal = Principal::User {
        id: "another-owner".into(),
        roles: vec![],
    };
    assert!(
        personal_workspace_scope(&context, WorkspacePurpose::Workspace)
            .await
            .is_err()
    );
    context.principal = original_principal;

    let original_identity = context.personal_dot.clone();
    context.personal_dot.as_mut().unwrap().dot_id = "another-dot".into();
    assert!(
        personal_workspace_scope(&context, WorkspacePurpose::Workspace)
            .await
            .is_err()
    );
    context.personal_dot = original_identity;

    let original_session = context.session_id.clone();
    let other_session = chat::create_session(&context.db, "workspace-owner")
        .await
        .unwrap();
    context.session_id = Some(other_session.id);
    assert!(
        personal_workspace_scope(&context, WorkspacePurpose::Workspace)
            .await
            .is_err()
    );
    context.session_id = original_session;

    let original_turn = context.assistant_turn_id.clone();
    context.assistant_turn_id = Some("another-assistant".into());
    assert!(
        personal_workspace_scope(&context, WorkspacePurpose::Workspace)
            .await
            .is_err()
    );
    context.assistant_turn_id = original_turn;
    assert!(
        personal_workspace_scope(&context, WorkspacePurpose::Workspace)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn workspace_scope_rechecks_current_dot_activation() {
    let context = running_context().await;
    let identity = context.personal_dot.as_ref().unwrap();
    assert!(
        agents::set_personal_dot_active(
            &context.db,
            &identity.dot_id,
            &identity.owner_user_id,
            false,
            4,
        )
        .await
        .unwrap()
    );
    for purpose in [WorkspacePurpose::Workspace, WorkspacePurpose::Browser] {
        assert!(personal_workspace_scope(&context, purpose).await.is_err());
    }
}

#[tokio::test]
async fn workspace_scope_rejects_completed_and_suspended_current_turns() {
    for status in [TurnStatus::Completed, TurnStatus::Suspended] {
        let context = running_context().await;
        chat::finalize_turn(
            &context.db,
            context.assistant_turn_id.as_deref().unwrap(),
            status,
            None,
        )
        .await
        .unwrap();
        for purpose in [WorkspacePurpose::Workspace, WorkspacePurpose::Browser] {
            assert!(personal_workspace_scope(&context, purpose).await.is_err());
        }
    }
}

#[tokio::test]
async fn workspace_scope_rechecks_the_private_session_binding() {
    let context = running_context().await;
    sqlx::query("UPDATE chat_sessions SET shared = 1 WHERE id = ?")
        .bind(context.session_id.as_deref().unwrap())
        .execute(&context.db)
        .await
        .unwrap();
    assert!(
        personal_workspace_scope(&context, WorkspacePurpose::Workspace)
            .await
            .is_err()
    );
}
