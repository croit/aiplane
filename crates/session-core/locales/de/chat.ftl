# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/mod.rs` — the
# multi-conversation chat page's server-side handlers: page title
# fallback, sidebar/effort/share/pin toasts, and the SSE-toast error
# messages the composer's fetch layer surfaces on failed actions.

chat-default-title = Chat

chat-error-still-streaming = Für diesen Benutzer läuft noch eine Antwort — bitte warten oder Stopp drücken.

chat-error-auth-required = Authentifizierung erforderlich
chat-error-no-such-turn = keine solche Nachricht
chat-error-db-error = Datenbankfehler
chat-error-attachments-not-configured = Chat-Anhänge sind nicht konfiguriert
chat-error-bad-filename = ungültiger Dateiname
chat-error-attachment-not-found = nicht gefunden

# SPA-only chat chrome (`web/src/routes/chat/*`): the conversation list,
# the conversation header, the assistant's ask-back card, and the
# turn-status line the server never renders itself.
chat-list-empty = Noch keine Unterhaltungen. Starte oben eine neue.
chat-turn-stopped = gestoppt
chat-prompt-heading = Der Assistent fragt
chat-prompt-placeholder = Antwort eingeben …
chat-prompt-answer = Antworten
chat-prompt-skip = Überspringen
