# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `session-core/src/render.rs` — the HTML renderers for
# the chat-style session UI (conversation bubbles, tool-call rows, the
# document canvas, and the composer). Driver-agnostic: both the gateway
# and any future consumer of this crate render through these functions.

render-edit-button = ✎ Modifier

render-retry-button = ↻ Réessayer

render-attachment-remove-aria = Supprimer la pièce jointe

# Bouton de copie sur un bloc de code d'une réponse (icône seule, donc
# ceci est son infobulle / nom accessible).

render-thinking-spinner = Réflexion…
render-thinking-finalized = Réflexion pendant { $secs } s

render-tool-status-used = Utilisé

render-canvas-edit-button = ✎ Modifier
render-canvas-save = Enregistrer comme nouvelle version
render-canvas-cancel = Annuler

render-composer-attach-aria = Joindre des fichiers
render-composer-attach-title = Joindre des fichiers (aussi par glisser-déposer / coller)
render-composer-send = Envoyer
render-composer-stop = Arrêter

# The browser prompt behind the ✎ Edit button, and the per-file title on
# an attachment's remove button.
render-edit-prompt = Modifiez votre message :
render-attachment-remove-title = Supprimer { $filename }
