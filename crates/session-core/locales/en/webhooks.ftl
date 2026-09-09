# Strings owned by `gateway/src/rama_server/pages/webhooks.rs` — the webhooks
# management page (create form + list), its full-page edit sub-page, and the
# one-time trigger-URL reveal.

webhooks-heading = Webhooks
webhooks-intro = Run a prompt when an external service calls a URL. You get a secret trigger URL; whatever the caller sends in the request body is appended to your prompt, and the run opens as a new chat you can read here.
webhooks-edit-heading = Edit webhook
webhooks-list-empty = No webhooks yet. Create one above.

webhooks-name-label = Name
webhooks-name-placeholder = e.g. Deploy digest
webhooks-model-label = Model
webhooks-model-placeholder = Model id
webhooks-prompt-placeholder = What should the model do with the incoming payload?

webhooks-reveal-heading = Your trigger URL
webhooks-reveal-note = Copy it now — it's shown only once. Anyone with this URL can fire the webhook. Lost it? Rotate to get a new one.
webhooks-copy = Copy

webhooks-badge-active = Active
webhooks-badge-paused = Paused
webhooks-mode-sync = Waits for response

webhooks-pause-title = Pause
webhooks-resume-title = Resume
webhooks-rotate-title = Rotate secret
webhooks-edit-title = Edit
webhooks-delete-title = Delete

# --- Rerun with a different prompt ---
webhooks-toast-rerun-started = Rerun complete — opening the conversation…

# --- Run history ---
webhooks-runs-empty = No runs yet. Fire the webhook to see its history here.
webhooks-run-open = open chat
webhooks-run-rerun = rerun

# SPA-only: the Svelte /webhooks page — inline form, run list, rerun composer.
webhooks-new-heading = New webhook
webhooks-prompt-untrusted-label = Prompt (the payload arrives as untrusted input)
webhooks-runs-show = Runs
webhooks-runs-hide = Hide runs
webhooks-rerun-prompt-label = Rerun prompt — the stored payload is replayed through this
webhooks-rerun-latest = Rerun latest payload
webhooks-rerun-running = Running…
webhooks-toast-rerun-failed = Rerun { $status }
webhooks-rotate-confirm = Issue a new trigger secret? The old URL stops working immediately.
webhooks-delete-confirm = Delete this webhook?
