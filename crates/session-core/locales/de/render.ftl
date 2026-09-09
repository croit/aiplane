# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `session-core/src/render.rs` — the HTML renderers for
# the chat-style session UI (conversation bubbles, tool-call rows, the
# document canvas, and the composer). Driver-agnostic: both the gateway
# and any future consumer of this crate render through these functions.

render-edit-button = ✎ Bearbeiten

render-retry-button = ↻ Wiederholen

render-attachment-remove-aria = Anhang entfernen

# Kopier-Schaltfläche an einem Codeblock in einer Antwort (nur Symbol,
# daher ist dies der Tooltip bzw. der zugängliche Name).

render-thinking-spinner = Denkt nach…
render-thinking-finalized = { $secs }s nachgedacht

render-tool-status-used = Verwendet

render-canvas-edit-button = ✎ Bearbeiten
render-canvas-save = Als neue Version speichern
render-canvas-cancel = Abbrechen

render-composer-attach-aria = Dateien anhängen
render-composer-attach-title = Dateien anhängen (auch per Ablegen/Einfügen)
render-composer-send = Senden
render-composer-stop = Stopp

# The browser prompt behind the ✎ Edit button, and the per-file title on
# an attachment's remove button.
render-edit-prompt = Deine Nachricht bearbeiten:
render-attachment-remove-title = { $filename } entfernen
