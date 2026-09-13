# The feedback widget: the floating button, the dialog (form, voice intake,
# screenshot annotator, attachments, diagnostics consent), the
# public-tracker confirmation, and the `/api/v0/feedback*` API errors.
#
# The chrome is rendered by the SPA (web/src/lib/components/feedback/); the
# `feedback-err-*` messages come from crates/gateway-api/src/pages/feedback.rs.

feedback-fab-aria = Send feedback
feedback-fab-title = Send feedback

feedback-dialog-heading = Send feedback
feedback-dialog-blurb = The page, your browser and the recent console and network activity are attached automatically.
feedback-close-aria = Close
feedback-cancel-button = Cancel
feedback-submit-button = Send feedback
feedback-sending = Sending…

feedback-title-label = Title
feedback-title-placeholder = Short summary
feedback-description-label = Description
feedback-description-placeholder = What happened, or what would you like?
feedback-business-label = Business value
feedback-business-placeholder = Why does this matter? Who is impacted?
feedback-acceptance-label = Acceptance criteria
feedback-acceptance-placeholder = When is this done?
feedback-priority-label = Priority
feedback-priority-low = Low
feedback-priority-medium = Medium
feedback-priority-high = High

# Voice intake — one recording fills every field above.
feedback-voice-button-label = Fill in by voice
feedback-voice-button-title = Tap, describe the issue, tap again — we'll fill in the fields below
feedback-voice-stop-label = Stop & fill in
feedback-voice-working-label = Transcribing…
feedback-voice-applied = Filled in from your recording — review and send.
feedback-voice-no-speech = No speech detected — try again.

# Screenshot + annotation.
feedback-shot-label = Screenshot
feedback-shot-status-capturing = Capturing…
feedback-shot-status-attached = Attached — draw on it to annotate
feedback-shot-status-none = No screenshot
feedback-shot-status-failed = Screenshot unavailable
feedback-shot-capture = Add a screenshot
feedback-shot-recapture = Recapture
feedback-shot-remove = Remove
feedback-shot-exact = Pixel-exact
feedback-shot-exact-title = Captures the real on-screen pixels through the browser's screen-share picker — includes canvas, WebGL and embedded frames the normal capture cannot reproduce.
feedback-shot-exact-cancelled = Screen capture cancelled — keeping the current screenshot.
feedback-shot-capture-failed = Could not capture a screenshot.

feedback-shot-annotate = Annotate
feedback-annotate-heading = Annotate the screenshot
feedback-annotate-done = Back to the form
feedback-annotate-hint = Drag to draw. Pick the move tool (or hold the middle mouse button) to pan a zoomed-in view; ctrl/⌘ + scroll zooms.
feedback-tool-pan-title = Move / pan
feedback-zoom-preset-title = Click to fit the whole screenshot, click again to fill the width
feedback-zoom-fit-label = Fit
feedback-zoom-width-label = Width

feedback-tool-rect-title = Rectangle
feedback-tool-arrow-title = Arrow
feedback-tool-pen-title = Freehand
feedback-tool-text-title = Text
feedback-tool-redact-title = Hide / redact (filled box)
feedback-tool-text-prompt = Annotation text
feedback-color-aria = Colour
feedback-undo-title = Undo
feedback-redo-title = Redo
feedback-clear-annot-title = Clear annotations
feedback-clear-annot-label = Clear
feedback-zoom-out-title = Zoom out
feedback-zoom-in-title = Zoom in

# Extra images, pasted or dropped onto the form.
feedback-attachments-label = Images
feedback-attachments-count = { $count } of { $max }
feedback-attachments-hint = Paste or drop images here to attach them.
feedback-attachments-drop = Drop to attach
feedback-attachments-remove = Remove image
feedback-attachments-too-many = At most { $max } images.
feedback-attachments-too-large = That image is larger than { $max } MB.
feedback-attachments-invalid = Only image files can be attached.

# Diagnostics consent. Both default to on; the chat log only appears on a
# conversation page.
feedback-log-browser-label = Submit browser activity log (console + network)
feedback-log-chat-label = Submit chat & tool usage log
feedback-diagnostics-label = Show the data that will be attached

feedback-confirm-heading = Are you sure?
feedback-confirm-public-p1-prefix = This feedback opens a ticket in our
feedback-confirm-public-p1-strong = public
feedback-confirm-public-p1-suffix = issue tracker. Anyone can read it.
feedback-confirm-private-p2-prefix = Please make sure your screenshot and the submitted data contain
feedback-confirm-private-p2-strong = no personal or private information
feedback-confirm-private-p2-suffix = (names, emails, tokens, customer data, …).
feedback-confirm-cancel-button = No, let me edit
feedback-confirm-ok-button = Yes, send

feedback-thanks-heading = Thank you
feedback-thanks-body = Your feedback was filed as an issue.
feedback-thanks-issue = Filed as issue #{ $number }.
feedback-thanks-open = Open the issue
feedback-done-button = Done

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
feedback-err-network = Network error: { $error }
