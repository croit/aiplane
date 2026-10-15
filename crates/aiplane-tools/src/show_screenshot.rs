// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `show_screenshot` — capture the user's browser and put the picture in the
//! reply.
//!
//! `browser_control` screenshots are for the model: they travel back as
//! `image_url` parts on the tool message, which the user never sees. That is
//! right for the dozen captures an agent takes to find its way around a page,
//! and wrong the moment the user asks to be *shown* something — the model said
//! "here is the screenshot" and the conversation held no image. This tool is
//! the other half: one capture, through the same extension and the same
//! rendezvous, stored as a chat attachment and spliced into the turn the way
//! `load_image_url` does, so it renders inline.
//!
//! A separate tool rather than a flag on the batch because presenting is a
//! capability of its own: it has a row on `/tools`, a key `enable_tools` can
//! turn on, and a switch a user can turn off without losing the browser.

use serde::Deserialize;
use serde_json::{Value, json};
use session_core::db as chat;
use session_core::workers::{BrowserAction, CaptureRegion};
use shared::api::ToolDef;

use aiplane_features::server::chat_attachments;
use aiplane_runtime::server::tools::feedback::BrowserReply;
use aiplane_runtime::server::tools::{Tool, ToolContext, ToolError, ToolFuture};

use crate::browser_control::{answer, audit, region_schema, request_actions, validate};

pub struct ShowScreenshot;

const AUDIT_ACTIONS: &[&str] = &["show_screenshot"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    full_page: bool,
    #[serde(default)]
    r#ref: Option<String>,
    #[serde(default)]
    region: Option<CaptureRegion>,
}

impl Tool for ShowScreenshot {
    fn id(&self) -> &str {
        "show_screenshot"
    }

    fn max_duration(&self) -> Option<std::time::Duration> {
        crate::browser_control::BrowserControl.max_duration()
    }

    fn schema(&self) -> ToolDef {
        let mut region = region_schema();
        region["description"] = json!(
            "A rectangle of the page in CSS pixels, from the top-left of the document. \
             A `browser_control` screenshot reports the area it covered as `clip`."
        );
        ToolDef::function(
            self.id(),
            "Show the user a screenshot of the page open in their browser (the tab \
             `browser_control` works in): the viewport, the whole page, one element, or a \
             rectangle of it. The image appears inline in your reply — this is the only way \
             the user sees a capture, because `browser_control` screenshots are visible to \
             you alone. Cut it to what matters: pass the `ref` of the element from a \
             `read_page` / `find` result, or a `region`. Do not repeat the returned marker or \
             describe the image as if the user could not see it; refer to it as the \
             screenshot above.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "full_page": {
                        "type": "boolean",
                        "description": "Capture the whole page, beyond the viewport."
                    },
                    "ref": {
                        "type": "string",
                        "description": "Capture just this element (a ref from `read_page` \
                                        or `find`), with a little margin around it."
                    },
                    "region": region
                }
            }),
        )
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: Args = serde_json::from_value(args).map_err(|e| {
                ToolError::InvalidArgs(format!(
                    "expected {{full_page?, ref?, region?: {{x, y, width, height}}}}: {e}"
                ))
            })?;
            let action = BrowserAction::Screenshot {
                full_page: args.full_page,
                r#ref: args.r#ref,
                region: args.region,
            };
            validate(&action)?;

            let (Some(fb), Some(turn_id)) =
                (ctx.chat_feedback.as_ref(), ctx.assistant_turn_id.as_deref())
            else {
                return Err(ToolError::Failed(
                    "show_screenshot only works inside a chat session — it captures the \
                     browser paired with the open conversation and attaches the image to \
                     your reply, and there is neither here"
                        .into(),
                ));
            };
            // Checked before the browser is touched: a capture that has nowhere
            // to go is a page the user's browser rendered for nothing.
            let (Some(s3), Some(reservations)) =
                (ctx.s3.as_ref(), ctx.attachment_reservations.as_ref())
            else {
                return Err(ToolError::Failed(
                    "chat attachments are not configured on this gateway (the operator must \
                     enable S3 attachment storage in /admin/settings), so there is nowhere to put the screenshot — describe \
                     what you see instead"
                        .into(),
                ));
            };

            let reply = request_actions(fb, turn_id, vec![action]).await;
            audit(&ctx, turn_id, AUDIT_ACTIONS, 0, reply.as_ref()).await;
            let Some(BrowserReply::Done { mut results }) = reply else {
                return Ok(answer(reply, AUDIT_ACTIONS));
            };

            let Some(shot) = results
                .first()
                .and_then(|r| r.get("screenshot"))
                .and_then(Value::as_str)
            else {
                return Err(ToolError::Failed(
                    "the browser extension answered without an image — it may be older than \
                     this gateway; ask the user to update it"
                        .into(),
                ));
            };
            let (mime, bytes) = chat_attachments::from_data_uri(shot)
                .map_err(|e| ToolError::Failed(format!("the screenshot did not decode: {e}")))?;
            let ext = chat_attachments::ext_for_mime(&mime).unwrap_or(".png");

            let filename = chat_attachments::reserve_filename(
                &ctx.db,
                turn_id,
                reservations,
                &format!("screenshot{ext}"),
            )
            .await
            .map_err(|e| ToolError::Failed(format!("reserve filename: {e}")))?;
            let outcome = chat_attachments::upload(s3, turn_id, &filename, &mime, bytes)
                .await
                .map_err(|e| {
                    ToolError::Failed(format!(
                        "storing the screenshot failed ({e}) — the user has not seen it"
                    ))
                })?;
            let marker = chat_attachments::marker_line(turn_id, &outcome);
            chat::append_content(&ctx.db, turn_id, &format!("\n\n{marker}\n\n"))
                .await
                .map_err(|e| ToolError::Failed(format!("persist marker: {e}")))?;

            results[0]["shown_to_user"] = json!(outcome.filename);
            results[0]["id"] = json!(format!("{turn_id}/{}", outcome.filename));
            // The model gets the same picture the user now has in front of
            // them, so what it says about the image is about this image.
            Ok(crate::browser_control::done(results, None))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiplane_core::server::db;
    use aiplane_runtime::server::tools::feedback::FeedbackHub;
    use aiplane_runtime::server::tools::{ChatFeedback, extract_content_parts};
    use session_core::workers::TurnUpdate;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct OnChat {
        ctx: ToolContext,
        hub: std::sync::Arc<FeedbackHub<BrowserReply>>,
        rx: tokio::sync::broadcast::Receiver<TurnUpdate>,
    }

    /// A chat turn `t1` in a real in-memory database, with the browser
    /// rendezvous wired and — when `s3` is given — an object store to put the
    /// capture in.
    async fn on_chat(s3: Option<&str>) -> OnChat {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let now = jiff::Timestamp::now();
        db::users::upsert(
            &pool,
            &db::users::User {
                id: "u".into(),
                email: "u@example.com".into(),
                name: None,
                roles: vec![],
                created_at: now,
                updated_at: now,
                timezone: None,
                speech_voice: None,
            },
        )
        .await
        .unwrap();
        let session = chat::create_session(&pool, "u").await.unwrap();
        chat::create_assistant_turn_in_progress(&pool, &session.id, "t1", "m")
            .await
            .unwrap();
        let (broadcast, rx) = tokio::sync::broadcast::channel(16);
        let hub = std::sync::Arc::new(FeedbackHub::<BrowserReply>::default());
        let mut ctx = ToolContext::for_test(pool);
        ctx.assistant_turn_id = Some("t1".into());
        ctx.session_id = Some(session.id);
        ctx.chat_feedback = Some(ChatFeedback {
            browser_hub: hub.clone(),
            ..ChatFeedback::for_test(broadcast)
        });
        if let Some(endpoint) = s3 {
            ctx.s3 = Some(std::sync::Arc::new(
                aiplane_core::server::config::S3Config {
                    endpoint: endpoint.into(),
                    region: "us-east-1".into(),
                    bucket: "b".into(),
                    access_key: Some("test-access".into()),
                    secret_key: Some("test-secret".into()),
                    key_prefix: "chat-attachments".into(),
                },
            ));
            ctx.attachment_reservations = Some(Default::default());
        }
        OnChat { ctx, hub, rx }
    }

    async fn next_request(
        rx: &mut tokio::sync::broadcast::Receiver<TurnUpdate>,
    ) -> (String, Vec<BrowserAction>) {
        loop {
            if let TurnUpdate::Browser(request) = rx.recv().await.expect("a browser request") {
                return (request.request_id.clone(), request.actions.clone());
            }
        }
    }

    #[test]
    fn schema_names_match_id() {
        assert_eq!(ShowScreenshot.id(), ShowScreenshot.schema().function.name);
    }

    #[tokio::test]
    async fn off_the_chat_path_it_refuses() {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        let err = ShowScreenshot
            .run(ToolContext::for_test(pool), json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("chat session"), "{err}");
    }

    #[tokio::test]
    async fn without_attachment_storage_the_browser_is_never_asked() {
        let OnChat { ctx, mut rx, .. } = on_chat(None).await;
        let err = ShowScreenshot.run(ctx, json!({})).await.unwrap_err();
        assert!(err.to_string().contains("S3 attachment storage"), "{err}");
        assert!(
            rx.try_recv().is_err(),
            "nothing may reach the browser when the image has nowhere to go"
        );
    }

    #[tokio::test]
    async fn two_areas_at_once_are_refused_before_anything_runs() {
        let OnChat { ctx, .. } = on_chat(None).await;
        let err = ShowScreenshot
            .run(ctx, json!({"full_page": true, "ref": "e1"}))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)), "{err:?}");
    }

    #[tokio::test]
    async fn the_capture_lands_in_the_reply_and_in_front_of_the_model() {
        let store = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&store)
            .await;
        let OnChat {
            ctx, hub, mut rx, ..
        } = on_chat(Some(&store.uri())).await;
        let db = ctx.db.clone();

        let call = tokio::spawn(async move { ShowScreenshot.run(ctx, json!({"ref": "e4"})).await });

        let (request_id, actions) = next_request(&mut rx).await;
        assert_eq!(
            actions,
            vec![BrowserAction::Screenshot {
                full_page: false,
                r#ref: Some("e4".into()),
                region: None,
            }]
        );
        let png = chat_attachments::to_data_uri("image/png", b"\x89PNG fake");
        assert!(hub.resolve(
            &request_id,
            BrowserReply::Done {
                results: vec![json!({"screenshot": png})],
            }
        ));

        let out = call.await.unwrap().expect("tool succeeds");
        let parts = extract_content_parts(&out).expect("the model sees the image too");
        assert_eq!(parts[1]["image_url"]["url"], png);
        assert!(
            parts[0]["text"]
                .as_str()
                .unwrap()
                .contains("screenshot.png"),
            "{out}"
        );

        let content = chat::get_content(&db, "t1")
            .await
            .unwrap()
            .unwrap_or_default();
        assert!(
            content.contains("screenshot.png"),
            "the reply must carry the attachment marker: {content}"
        );
        let put = &store.received_requests().await.unwrap()[0];
        assert!(
            put.url.path().ends_with("/t1/screenshot.png"),
            "{}",
            put.url
        );
    }

    #[tokio::test]
    async fn a_second_capture_in_one_reply_gets_its_own_name() {
        let store = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&store)
            .await;
        let OnChat {
            ctx, hub, mut rx, ..
        } = on_chat(Some(&store.uri())).await;
        let png = chat_attachments::to_data_uri("image/png", b"\x89PNG fake");

        let mut names = Vec::new();
        for _ in 0..2 {
            let ctx = ctx.clone();
            let call = tokio::spawn(async move { ShowScreenshot.run(ctx, json!({})).await });
            let (request_id, _) = next_request(&mut rx).await;
            hub.resolve(
                &request_id,
                BrowserReply::Done {
                    results: vec![json!({"screenshot": png})],
                },
            );
            let out = call.await.unwrap().unwrap();
            let text = extract_content_parts(&out).unwrap()[0]["text"]
                .as_str()
                .unwrap()
                .to_string();
            let body: Value = serde_json::from_str(&text).unwrap();
            names.push(
                body["results"][0]["shown_to_user"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            );
        }
        assert_ne!(names[0], names[1], "{names:?}");
    }

    #[tokio::test]
    async fn a_refusal_is_passed_on_as_an_answer() {
        let store = MockServer::start().await;
        let OnChat {
            ctx, hub, mut rx, ..
        } = on_chat(Some(&store.uri())).await;
        let call = tokio::spawn(async move { ShowScreenshot.run(ctx, json!({})).await });
        let (request_id, _) = next_request(&mut rx).await;
        hub.resolve(
            &request_id,
            BrowserReply::Refused {
                reason: "not approved".into(),
            },
        );
        let out = call.await.unwrap().unwrap();
        assert_eq!(out["refused"], true, "{out}");
        assert!(store.received_requests().await.unwrap().is_empty());
    }
}
