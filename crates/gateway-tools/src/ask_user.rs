// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `ask_user` — ask the user a question mid-turn and wait for the answer.
//!
//! Without this the model has two bad options when a request is
//! underspecified: guess (and produce work that gets thrown away), or answer
//! with a question and end the turn — which tears down everything the turn had
//! built up. A sandbox lease with a populated `/work`, a canvas document in
//! progress, files staged for a render: all cold by the next message. Asking
//! *inside* the turn keeps the work alive.
//!
//! Mechanically it is the same rendezvous `get_user_location` uses: inject a
//! card onto the live SSE stream, park on the feedback hub keyed by the
//! assistant turn id, and wait for `POST /api/v0/me/ask/feedback/{turn_id}`.
//! See `gateway_runtime::server::tools::feedback`.
//!
//! Chat-path only. Off it there is no browser to ask and no turn to attach a
//! card to, so the tool refuses rather than hanging — and `requires_chat_session`
//! keeps it out of the `/v1` tool list entirely.

use serde::Deserialize;
use serde_json::{Value, json};
use shared::api::ToolDef;

use gateway_runtime::server::tools::feedback::AskReply;
use gateway_runtime::server::tools::{ChatFeedback, Tool, ToolContext, ToolError, ToolFuture};
use session_core::workers::PreviewKind;

/// How long to wait for a human to answer.
///
/// Far longer than the location prompt's wait: that one is answered by the
/// browser in a second or two once the user clicks, whereas this one waits for
/// someone to read a question, think, and possibly type. Short enough that a
/// user who wandered off doesn't pin a worker indefinitely.
const WAIT_SECS: u64 = 180;

/// Ceiling on the per-tool timeout the runner enforces. Must exceed
/// [`WAIT_SECS`] or the runner would cancel the tool while it is still
/// legitimately waiting — the default 30s would.
const MAX_DURATION_SECS: u64 = WAIT_SECS + 15;

/// Bounds on the question itself. A tool call is not the place to render an
/// essay, and an unbounded option list would be a broken UI.
const MAX_QUESTION_LEN: usize = 1_000;
const MAX_HEADER_LEN: usize = 40;
const MAX_OPTIONS: usize = 4;
const MAX_LABEL_LEN: usize = 80;
const MAX_DESCRIPTION_LEN: usize = 400;
/// Previews get their own, larger budget: an ASCII layout or a code snippet
/// needs room that a one-line description does not, and the same ceiling for
/// both would either starve the diagram or invite an essay in the subtitle.
const MAX_PREVIEW_LEN: usize = 1_200;

pub struct AskUser;

#[derive(Deserialize)]
struct AskArgs {
    question: String,
    #[serde(default)]
    header: Option<String>,
    #[serde(default)]
    options: Option<Vec<AskOption>>,
    #[serde(default)]
    multi_select: bool,
}

#[derive(Deserialize, Clone, Debug)]
struct AskOption {
    label: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    preview: Option<AskPreview>,
}

#[derive(Deserialize, Clone, Debug)]
struct AskPreview {
    #[serde(rename = "type")]
    kind: String,
    content: String,
}

impl Tool for AskUser {
    fn id(&self) -> &str {
        "ask_user"
    }

    fn max_duration(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_secs(MAX_DURATION_SECS))
    }

    fn schema(&self) -> ToolDef {
        ToolDef::function(
            self.id(),
            "Ask the user a question and wait for their answer, without ending your turn. \
             Use it when a genuine choice would change what you build and guessing would \
             waste the work — which database to target, which of several files they meant, \
             whether to overwrite something. Offer `options` when you can enumerate the \
             sensible answers; the user can always type something else instead. \
             \
             Do NOT use it to confirm things you can just do, to ask permission for a \
             normal step, or to check in on progress — a question the user has to answer \
             costs them far more than a wrong guess you can correct. Ask once, with \
             everything you need, rather than several times in a row. If nobody answers, \
             the result says so and you should proceed on a stated assumption.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["question"],
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The question, in the user's language. One question, \
                                        phrased so the options (if any) are obvious answers \
                                        to it. Markdown is rendered, so `code`, **bold** and \
                                        short lists work; keep it to a question, not a \
                                        briefing."
                    },
                    "header": {
                        "type": "string",
                        "description": "Optional short label for the card (a few words, e.g. \
                                        \"Target database\") so the user sees at a glance \
                                        what is being decided."
                    },
                    "options": {
                        "type": "array",
                        "maxItems": MAX_OPTIONS,
                        "description": "Optional list of 2-4 answers to offer as buttons. \
                                        Omit for an open question. A free-text field is \
                                        always shown as well, so never add an \"other\" \
                                        option yourself.",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["label"],
                            "properties": {
                                "label": {
                                    "type": "string",
                                    "description": "Short answer text (1-5 words). This is \
                                                    what comes back to you."
                                },
                                "description": {
                                    "type": "string",
                                    "description": "Optional one-line explanation of what \
                                                    choosing this means. Markdown is rendered."
                                },
                                "preview": {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "required": ["type", "content"],
                                    "description": "Optional worked example of what this \
                                                    option produces, shown inside the option. \
                                                    Use it when the choice is easier to see \
                                                    than to describe — a layout, a diagram, \
                                                    the shape of some output. Skip it when \
                                                    the label and description already say it.",
                                    "properties": {
                                        "type": {
                                            "type": "string",
                                            "enum": ["text", "svg"],
                                            "description": "`text` for an ASCII diagram, a \
                                                            table sketch or a code snippet — \
                                                            shown verbatim in a monospace \
                                                            block, so line art lines up. \
                                                            `svg` for a small inline drawing."
                                        },
                                        "content": {
                                            "type": "string",
                                            "maxLength": MAX_PREVIEW_LEN,
                                            "description": "The preview body. For `svg`, one \
                                                            complete <svg> element with a \
                                                            viewBox and no scripts, links or \
                                                            external references — those are \
                                                            stripped before it is shown. Use \
                                                            currentColor for strokes and fills \
                                                            so it works on light and dark."
                                        }
                                    }
                                }
                            }
                        }
                    },
                    "multi_select": {
                        "type": "boolean",
                        "description": "Allow picking several options at once. Default false. \
                                        Only set it when the options genuinely combine."
                    }
                }
            }),
        )
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: AskArgs = serde_json::from_value(args).map_err(|e| {
                ToolError::InvalidArgs(format!(
                    "expected {{question: string, header?, options?, multi_select?}}: {e}"
                ))
            })?;
            let prompt = Prompt::validate(args)?;

            // Chat path only: without a live turn there is no card to inject
            // and nobody to answer.
            let (Some(fb), Some(turn_id)) =
                (ctx.chat_feedback.as_ref(), ctx.assistant_turn_id.as_deref())
            else {
                return Err(ToolError::Failed(
                    "ask_user only works inside a chat session — there is no user watching \
                     this request to answer. Proceed on your best assumption and say which \
                     assumption you made."
                        .into(),
                ));
            };

            match request_answer(fb, turn_id, &prompt).await {
                Some(AskReply::Answered { choices, text }) => Ok(json!({
                    "answered": true,
                    "choices": choices,
                    "text": text,
                })),
                Some(AskReply::Dismissed) => Ok(unanswered(
                    "dismissed",
                    "The user dismissed the question without answering.",
                )),
                None => Ok(unanswered(
                    "no_response",
                    "Nobody answered in time (or nobody was watching this conversation).",
                )),
            }
        })
    }
}

/// How a [`confirm`] prompt came back.
#[derive(Debug, PartialEq, Eq)]
pub enum Confirmation {
    /// The user picked the affirmative option.
    Approved,
    /// The user picked the negative option, dismissed the card, or typed
    /// something instead of choosing.
    Declined {
        /// What they said, when they typed rather than clicked — worth
        /// relaying so the model can act on "not Mondays, Tuesdays".
        text: Option<String>,
    },
    /// Nobody answered in time, or nobody was watching.
    NoAnswer,
}

/// Ask the user to confirm an action, reusing `ask_user`'s card + rendezvous.
///
/// Exists so a tool with a persistent side effect can require a human "yes"
/// without reimplementing the SSE card and the hub wait. `schedule_action` is
/// the first caller: an action created from injected text would otherwise run
/// **as the user**, on a schedule, indefinitely — persistence is what makes
/// prompt injection there worth a confirmation step, where an ordinary tool
/// call isn't.
///
/// Returns [`Confirmation::NoAnswer`] off the chat path rather than erroring,
/// so the caller decides what "no human here" means for its own operation.
pub async fn confirm(
    ctx: &ToolContext,
    question: &str,
    header: &str,
    approve_label: &str,
    decline_label: &str,
) -> Confirmation {
    let (Some(fb), Some(turn_id)) = (ctx.chat_feedback.as_ref(), ctx.assistant_turn_id.as_deref())
    else {
        return Confirmation::NoAnswer;
    };
    let prompt = Prompt {
        question: question.to_string(),
        header: Some(header.to_string()),
        options: vec![
            Choice {
                label: approve_label.to_string(),
                description: None,
                preview: None,
            },
            Choice {
                label: decline_label.to_string(),
                description: None,
                preview: None,
            },
        ],
        multi_select: false,
    };
    match request_answer(fb, turn_id, &prompt).await {
        Some(AskReply::Answered { choices, text }) => {
            // A click on the affirmative button is the only "yes". Free text
            // is never read as approval: "yes but move it to 07:00" is a
            // change request, and treating it as consent would write the
            // wrong thing.
            if choices.iter().any(|c| c == approve_label) {
                Confirmation::Approved
            } else {
                Confirmation::Declined {
                    text: text.filter(|t| !t.trim().is_empty()),
                }
            }
        }
        Some(AskReply::Dismissed) => Confirmation::Declined { text: None },
        None => Confirmation::NoAnswer,
    }
}

/// A validated question, ready to render.
#[derive(Debug)]
struct Prompt {
    question: String,
    header: Option<String>,
    options: Vec<Choice>,
    multi_select: bool,
}

/// One validated option.
#[derive(Debug, Clone)]
struct Choice {
    label: String,
    description: Option<String>,
    preview: Option<Preview>,
}

/// A validated preview block.
#[derive(Debug, Clone)]
struct Preview {
    kind: PreviewKind,
    content: String,
}

impl Preview {
    /// Check what the *server* can check: the kind is one we render, the body
    /// is non-empty and bounded, and an `svg` really is an SVG element.
    ///
    /// Deliberately not a blocklist of dangerous SVG constructs. Pattern-
    /// matching for `<script` or `onload=` is the kind of filter that looks
    /// like security and loses to the first novel encoding; the real gate is
    /// the client's sanitiser, which parses the document rather than reading
    /// it. What belongs here is what a parser downstream cannot recover from:
    /// size, and content that is not the shape it claims to be.
    fn validate(raw: AskPreview) -> Result<Self, ToolError> {
        let kind = match raw.kind.trim() {
            "text" => PreviewKind::Text,
            "svg" => PreviewKind::Svg,
            other => {
                return Err(ToolError::InvalidArgs(format!(
                    "preview type must be `text` or `svg` (got `{other}`)"
                )));
            }
        };
        // Only the ends are trimmed: interior whitespace is the diagram.
        let content = raw.content.trim_matches(['\n', '\r']).to_string();
        if content.trim().is_empty() {
            return Err(ToolError::InvalidArgs(
                "a preview must have content, or be left out".into(),
            ));
        }
        if content.chars().count() > MAX_PREVIEW_LEN {
            return Err(ToolError::InvalidArgs(format!(
                "preview is too long; keep it under {MAX_PREVIEW_LEN} characters"
            )));
        }
        if kind == PreviewKind::Svg && !content.trim_start().starts_with("<svg") {
            return Err(ToolError::InvalidArgs(
                "an `svg` preview must be a single <svg> element".into(),
            ));
        }
        Ok(Self { kind, content })
    }
}

impl Prompt {
    fn validate(args: AskArgs) -> Result<Self, ToolError> {
        let question = args.question.trim().to_string();
        if question.is_empty() {
            return Err(ToolError::InvalidArgs("question must not be empty".into()));
        }
        if question.chars().count() > MAX_QUESTION_LEN {
            return Err(ToolError::InvalidArgs(format!(
                "question too long; keep it under {MAX_QUESTION_LEN} characters"
            )));
        }
        let header = args
            .header
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty());
        if let Some(h) = &header
            && h.chars().count() > MAX_HEADER_LEN
        {
            return Err(ToolError::InvalidArgs(format!(
                "header too long; keep it under {MAX_HEADER_LEN} characters"
            )));
        }

        let mut options = Vec::new();
        for opt in args.options.unwrap_or_default() {
            let label = opt.label.trim().to_string();
            // A blank label would render an unclickable button and come back
            // as an empty answer.
            if label.is_empty() {
                return Err(ToolError::InvalidArgs(
                    "every option needs a non-empty label".into(),
                ));
            }
            if label.chars().count() > MAX_LABEL_LEN {
                return Err(ToolError::InvalidArgs(format!(
                    "option label `{label}` is too long; keep labels under {MAX_LABEL_LEN} \
                     characters"
                )));
            }
            let description = opt
                .description
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty());
            if let Some(d) = &description
                && d.chars().count() > MAX_DESCRIPTION_LEN
            {
                return Err(ToolError::InvalidArgs(format!(
                    "option description for `{label}` is too long; keep it under \
                     {MAX_DESCRIPTION_LEN} characters"
                )));
            }
            let preview = opt.preview.map(Preview::validate).transpose()?;
            options.push(Choice {
                label,
                description,
                preview,
            });
        }
        if options.len() > MAX_OPTIONS {
            return Err(ToolError::InvalidArgs(format!(
                "at most {MAX_OPTIONS} options (got {})",
                options.len()
            )));
        }
        // One option is a yes/no dressed up as a choice — the free-text field
        // is already the "something else" path, so a single button adds nothing
        // but a misleading UI.
        if options.len() == 1 {
            return Err(ToolError::InvalidArgs(
                "offer either no options (open question) or at least two".into(),
            ));
        }

        Ok(Self {
            question,
            header,
            options,
            multi_select: args.multi_select,
        })
    }
}

/// The shared shape for "we got no usable answer".
///
/// Deliberately a successful result with `answered: false` rather than an
/// error: the model's next step is the same either way (assume and say so),
/// and returning an error tends to make models retry the question.
fn unanswered(reason: &str, note: &str) -> Value {
    json!({
        "answered": false,
        "reason": reason,
        "note": format!("{note} Continue with your best assumption and state it explicitly \
                         in your reply so the user can correct you."),
    })
}

/// Announce the question, park on the hub, take the prompt down again.
///
/// The prompt travels as a structured `TurnUpdate::Prompt` event: the client
/// owns how it looks, the server only says what is being asked and by which
/// turn. `Hide` is sent whatever the outcome — the client removes its own
/// prompt on submit, so this covers the timeout and the give-up path.
async fn request_answer(fb: &ChatFeedback, turn_id: &str, prompt: &Prompt) -> Option<AskReply> {
    use session_core::workers::{
        PromptOption, PromptPreview, ToolPrompt, ToolPromptEvent, ToolPromptKind, TurnUpdate,
    };

    // Nobody subscribed → nobody can answer. The timeout below is the real
    // backstop if the stream drops right after this check.
    if fb.broadcast.receiver_count() == 0 {
        return None;
    }

    let rx = fb.ask_hub.register(turn_id);

    let _ = fb.broadcast.send(TurnUpdate::Prompt(std::sync::Arc::new(
        ToolPromptEvent::Show(ToolPrompt {
            turn_id: turn_id.to_string(),
            kind: ToolPromptKind::AskUser,
            question: prompt.question.clone(),
            // Label and description stay separate all the way to the card.
            // Joining them here made the button read as a sentence and, worse,
            // made that sentence the answer the model got back.
            options: prompt
                .options
                .iter()
                .map(|o| PromptOption {
                    label: o.label.clone(),
                    description: o.description.clone(),
                    preview: o.preview.as_ref().map(|p| PromptPreview {
                        kind: p.kind,
                        content: p.content.clone(),
                    }),
                })
                .collect(),
            header: prompt.header.clone(),
            multi_select: prompt.multi_select,
        }),
    )));

    let outcome = tokio::time::timeout(std::time::Duration::from_secs(WAIT_SECS), rx).await;

    // Tear down regardless of how the wait ended. The client removes its own
    // prompt on submit; this covers timeout and dismissal-by-navigation.
    let _ = fb.broadcast.send(TurnUpdate::Prompt(std::sync::Arc::new(
        ToolPromptEvent::Hide {
            turn_id: turn_id.to_string(),
        },
    )));

    match outcome {
        Ok(Ok(reply)) => Some(reply),
        // Timed out, or the sender was dropped.
        _ => {
            fb.ask_hub.cancel(turn_id);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::server::db;

    fn args(v: Value) -> AskArgs {
        serde_json::from_value(v).expect("valid args")
    }

    async fn ctx_off_chat() -> ToolContext {
        let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
        ToolContext::for_test(pool)
    }

    #[test]
    fn schema_names_match_id() {
        assert_eq!(AskUser.id(), AskUser.schema().function.name);
    }

    /// The runner's default per-tool timeout is 30s — far below the human wait,
    /// so without the override the tool would be cancelled mid-question.
    #[test]
    fn max_duration_outlasts_the_human_wait() {
        let d = AskUser.max_duration().expect("must override the default");
        assert!(
            d.as_secs() > WAIT_SECS,
            "{}s must exceed the {WAIT_SECS}s wait",
            d.as_secs()
        );
    }

    #[test]
    fn schema_advertises_the_option_bounds() {
        let schema = AskUser.schema();
        let props = &schema.function.parameters["properties"];
        assert_eq!(props["options"]["maxItems"], MAX_OPTIONS);
        assert_eq!(props["multi_select"]["type"], "boolean");
        assert_eq!(schema.function.parameters["required"], json!(["question"]));
    }

    #[test]
    fn rejects_an_empty_question() {
        assert!(matches!(
            Prompt::validate(args(json!({"question": "   "}))).unwrap_err(),
            ToolError::InvalidArgs(_)
        ));
    }

    #[test]
    fn rejects_a_single_option() {
        // One button plus a free-text field is a misleading UI, not a choice.
        let err = Prompt::validate(args(json!({
            "question": "Which?",
            "options": [{"label": "only one"}]
        })))
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)), "{err:?}");
    }

    #[test]
    fn rejects_a_blank_option_label() {
        assert!(matches!(
            Prompt::validate(args(json!({
                "question": "Which?",
                "options": [{"label": " "}, {"label": "b"}]
            })))
            .unwrap_err(),
            ToolError::InvalidArgs(_)
        ));
    }

    #[test]
    fn rejects_too_many_options() {
        let many: Vec<Value> = (0..MAX_OPTIONS + 1)
            .map(|i| json!({"label": format!("opt{i}")}))
            .collect();
        assert!(matches!(
            Prompt::validate(args(json!({"question": "Which?", "options": many}))).unwrap_err(),
            ToolError::InvalidArgs(_)
        ));
    }

    #[test]
    fn accepts_an_open_question_and_a_normal_choice() {
        let open = Prompt::validate(args(json!({"question": "Which region?"}))).unwrap();
        assert!(open.options.is_empty());
        assert!(!open.multi_select);

        let choice = Prompt::validate(args(json!({
            "question": "Which database?",
            "header": "Target",
            "multi_select": true,
            "options": [
                {"label": "Postgres", "description": "the primary"},
                {"label": "SQLite"}
            ]
        })))
        .unwrap();
        assert_eq!(choice.options.len(), 2);
        assert_eq!(choice.header.as_deref(), Some("Target"));
        assert!(choice.multi_select);
    }

    #[tokio::test]
    async fn off_the_chat_path_it_refuses_with_actionable_advice() {
        // /v1 callers have no browser. requires_chat_session should keep the
        // tool out of their list entirely, but the runtime gate has to hold
        // regardless — and tell the model what to do instead of retrying.
        let err = AskUser
            .run(ctx_off_chat().await, json!({"question": "Which one?"}))
            .await
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("assumption"), "{msg}");
    }

    /// The whole wiring in one test: the card the client receives keeps each
    /// option's label and description apart, and the answer the model gets
    /// back is the label alone.
    ///
    /// They used to be joined into one string before the broadcast, which cost
    /// on both ends — a row of buttons labelled with whole sentences, and a
    /// `choices` entry the model had to read as an answer even though half of
    /// it was the UI's own explanatory text.
    #[tokio::test]
    async fn the_card_carries_label_and_description_apart_and_answers_by_label() {
        use gateway_runtime::server::tools::feedback::FeedbackHub;
        use session_core::workers::{ToolPromptEvent, TurnUpdate};

        let (broadcast, mut rx) = tokio::sync::broadcast::channel(16);
        let ask_hub = std::sync::Arc::new(FeedbackHub::<AskReply>::default());
        let mut ctx = ctx_off_chat().await;
        ctx.assistant_turn_id = Some("t1".into());
        ctx.chat_feedback = Some(ChatFeedback {
            broadcast,
            hub: std::sync::Arc::new(FeedbackHub::default()),
            ask_hub: ask_hub.clone(),
            secure: true,
        });

        let call = tokio::spawn(async move {
            AskUser
                .run(
                    ctx,
                    json!({
                        "question": "Which database?",
                        "options": [
                            {"label": "Postgres", "description": "the primary"},
                            {"label": "SQLite"}
                        ]
                    }),
                )
                .await
        });

        let shown = loop {
            match rx.recv().await.expect("the show event") {
                TurnUpdate::Prompt(event) => match &*event {
                    ToolPromptEvent::Show(p) => break p.clone(),
                    ToolPromptEvent::Hide { .. } => panic!("hidden before it was shown"),
                },
                _ => continue,
            }
        };
        assert_eq!(shown.options[0].label, "Postgres");
        assert_eq!(shown.options[0].description.as_deref(), Some("the primary"));
        assert_eq!(shown.options[1].label, "SQLite");
        assert_eq!(shown.options[1].description, None);

        // The client answers with the label it was given, never the prose.
        while !ask_hub.resolve(
            "t1",
            AskReply::Answered {
                choices: vec![shown.options[0].label.clone()],
                text: None,
            },
        ) {
            tokio::task::yield_now().await;
        }
        let result = call.await.expect("the tool task").expect("answered");
        assert_eq!(result["answered"], true);
        assert_eq!(result["choices"], json!(["Postgres"]));
    }

    #[test]
    fn a_text_preview_keeps_its_interior_whitespace() {
        // The columns of an ASCII diagram are made of runs of spaces. Trim the
        // ends of the block, never the inside.
        let p = Prompt::validate(args(json!({
            "question": "Which layout?",
            "options": [
                {"label": "Split", "preview": {"type": "text", "content": "\n a | b \n---+---\n c | d \n"}},
                {"label": "Stacked"}
            ]
        })))
        .unwrap();
        let preview = p.options[0].preview.as_ref().expect("a preview");
        assert_eq!(preview.kind, PreviewKind::Text);
        assert_eq!(preview.content, " a | b \n---+---\n c | d ");
        assert!(p.options[1].preview.is_none());
    }

    #[test]
    fn an_svg_preview_must_actually_be_an_svg() {
        let err = Prompt::validate(args(json!({
            "question": "Which shape?",
            "options": [
                {"label": "Circle", "preview": {"type": "svg", "content": "<div>nope</div>"}},
                {"label": "Square"}
            ]
        })))
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)), "{err:?}");

        let ok = Prompt::validate(args(json!({
            "question": "Which shape?",
            "options": [
                {"label": "Circle", "preview": {"type": "svg", "content": "  <svg viewBox=\"0 0 8 8\"><circle r=\"3\"/></svg>"}},
                {"label": "Square"}
            ]
        })))
        .unwrap();
        assert_eq!(
            ok.options[0].preview.as_ref().unwrap().kind,
            PreviewKind::Svg
        );
    }

    #[test]
    fn rejects_an_unknown_preview_type_and_an_oversized_one() {
        let err = Prompt::validate(args(json!({
            "question": "Which?",
            "options": [
                {"label": "a", "preview": {"type": "html", "content": "<b>x</b>"}},
                {"label": "b"}
            ]
        })))
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)), "{err:?}");

        let huge = "x".repeat(MAX_PREVIEW_LEN + 1);
        let err = Prompt::validate(args(json!({
            "question": "Which?",
            "options": [{"label": "a", "preview": {"type": "text", "content": huge}}, {"label": "b"}]
        })))
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)), "{err:?}");
    }

    /// The server does not try to filter dangerous SVG constructs — that is
    /// the client sanitiser's job, and a regex that thinks it can do it would
    /// be worse than nothing because it would look like protection. This test
    /// pins that division so nobody "fixes" it into a blocklist: hostile
    /// markup is accepted here, bounded and typed, and disarmed downstream.
    #[test]
    fn hostile_svg_is_passed_on_for_the_client_sanitiser_not_pattern_matched() {
        let p = Prompt::validate(args(json!({
            "question": "Which?",
            "options": [
                {"label": "a", "preview": {"type": "svg", "content": "<svg><script>alert(1)</script></svg>"}},
                {"label": "b"}
            ]
        })))
        .expect("accepted, to be sanitised where a parser is available");
        assert!(
            p.options[0]
                .preview
                .as_ref()
                .unwrap()
                .content
                .contains("script")
        );
    }

    #[test]
    fn unanswered_results_tell_the_model_to_assume_and_say_so() {
        for v in [
            unanswered("dismissed", "The user dismissed it."),
            unanswered("no_response", "Nobody answered."),
        ] {
            assert_eq!(v["answered"], false);
            let note = v["note"].as_str().unwrap();
            assert!(note.contains("assumption"), "{note}");
        }
    }
}
