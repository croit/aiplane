# Strings owned by `session-core/src/render.rs` — the HTML renderers for
# the chat-style session UI (conversation bubbles, tool-call rows, the
# document canvas, and the composer). Driver-agnostic: both the gateway
# and any future consumer of this crate render through these functions.

render-edit-button = ✎ Edit

render-retry-button = ↻ Retry

render-attachment-remove-aria = Remove attachment

# Copy-to-clipboard button on a fenced code block in a reply (icon-only,
# so this is its tooltip / accessible name).

render-thinking-spinner = Thinking…
render-thinking-finalized = Thought for { $secs }s

render-tool-status-used = Used

render-canvas-edit-button = ✎ Edit
render-canvas-save = Save as new version
render-canvas-cancel = Cancel

render-composer-attach-aria = Attach files
render-composer-attach-title = Attach files (also drop / paste)
render-composer-send = Send
render-composer-stop = Stop

# The browser prompt behind the ✎ Edit button, and the per-file title on
# an attachment's remove button.
render-edit-prompt = Edit your message:
render-attachment-remove-title = Remove { $filename }
