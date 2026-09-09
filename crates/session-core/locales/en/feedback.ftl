# Strings owned by `gateway/src/rama_server/pages/feedback.rs` — the
# feedback-widget FAB + dialog + confirm-dialog UI chrome, and the
# `/feedback*` JSON API error messages.

feedback-fab-aria = Send feedback

feedback-dialog-heading = Send feedback
feedback-close-aria = Close

feedback-title-placeholder = Short summary
feedback-description-placeholder = What happened, or what would you like?
feedback-business-placeholder = Why does this matter? Who is impacted?
feedback-acceptance-placeholder = When is this done?
feedback-priority-low = Low
feedback-priority-medium = Medium
feedback-priority-high = High

feedback-cancel-button = Cancel
feedback-submit-button = Send feedback

feedback-err-no-session = No active session
feedback-err-session-lookup-failed = Session lookup failed
feedback-err-body-read = { $error }
feedback-err-empty-transcript = Empty transcript
feedback-err-malformed-json = Malformed JSON: { $error }
feedback-err-no-chat-model = No chat model available for extraction
feedback-err-extraction-failed = Extraction failed: { $error }
feedback-err-not-configured = Feedback is not configured
feedback-err-title-required = Title is required (at least 4 characters)
feedback-err-description-required = Description is required
feedback-err-submit-failed = Could not file the issue — please try again

# The SPA keeps the dialog open after submitting and shows a thank-you
# panel in place of the form; the server-rendered widget did not.
feedback-thanks-heading = Thank you
feedback-thanks-body = Your feedback was filed as an issue.
feedback-done-button = Done
feedback-sending = Sending…
