// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! How the `/api/v0` agent routes name a refused agent operation.

use rama::http::{Response, StatusCode};

use super::{internal, json_error};
use aiplane_runtime::agents::resume::AgentResumeError;
use aiplane_runtime::suspend::ResumeRefused;
use session_core::i18n::{Lang, t};

/// A refused resume, in the `/api/v0` envelope. Shared by the staff,
/// inbox and visitor routes, so all name a refusal the same way.
pub(super) fn resume_error(err: AgentResumeError, lang: Lang) -> Response {
    let (status, code) = match &err {
        AgentResumeError::Busy => {
            return json_error(
                StatusCode::CONFLICT,
                "turn_in_progress",
                &t(lang, "agent-communication-error-busy"),
            );
        }
        AgentResumeError::Unavailable => {
            return json_error(
                StatusCode::CONFLICT,
                "agent_unavailable",
                &t(lang, "agent-communication-error-unavailable"),
            );
        }
        AgentResumeError::Refused(ResumeRefused::NotSuspended)
        | AgentResumeError::Refused(ResumeRefused::StaleRequest { .. }) => {
            (StatusCode::CONFLICT, "not_suspended")
        }
        AgentResumeError::Refused(ResumeRefused::NotOffered { .. }) => {
            (StatusCode::BAD_REQUEST, "decision_not_offered")
        }
        AgentResumeError::StaffOnly { .. } => (StatusCode::FORBIDDEN, "decision_for_staff"),
        AgentResumeError::ParticipantOnly { .. } => (StatusCode::FORBIDDEN, "decision_for_visitor"),
        AgentResumeError::Refused(ResumeRefused::Storage(_)) | AgentResumeError::Db(_) => {
            return internal(err);
        }
    };
    json_error(status, code, &err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use session_core::i18n::{Lang, t};

    #[tokio::test]
    async fn personal_resume_refusals_use_the_requested_language_and_existing_error_envelope() {
        for lang in [Lang::En, Lang::De] {
            for (error, code, key) in [
                (
                    AgentResumeError::Busy,
                    "turn_in_progress",
                    "agent-communication-error-busy",
                ),
                (
                    AgentResumeError::Unavailable,
                    "agent_unavailable",
                    "agent-communication-error-unavailable",
                ),
            ] {
                let response = resume_error(error, lang);
                assert_eq!(response.status(), StatusCode::CONFLICT);
                let bytes = session_core::chrome::read_body_capped(response.into_body(), 16 * 1024)
                    .await
                    .unwrap();
                let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_ne!(t(lang, key), key);
                assert_eq!(body["error"]["message"], t(lang, key));
                assert_eq!(body["error"]["code"], code);
            }
        }
    }
}
