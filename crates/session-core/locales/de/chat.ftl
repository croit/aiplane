# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/mod.rs` — the
# multi-conversation chat page's server-side handlers: page title
# fallback, sidebar/effort/share/pin toasts, and the SSE-toast error
# messages the composer's fetch layer surfaces on failed actions.

chat-default-title = Chat

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

# Der Eingabebereich bleibt während einer laufenden Antwort nutzbar:
# Getipptes wartet in einer Warteschlange, ein Zwischenruf geht in die
# laufende Antwort, und die Antwort lässt sich abbrechen und neu ansetzen.
chat-queue-label = { $count ->
    [one] { $count } Nachricht wartet
   *[other] { $count } Nachrichten warten
  }
chat-queue-move-up = Nach oben
chat-queue-move-down = Nach unten
chat-queue-edit = Zurück in die Eingabe
chat-queue-remove = Verwerfen
chat-queue-held-title = Der Anhang ist beim Neuladen verloren gegangen — erneut anhängen oder Nachricht verwerfen
chat-queue-held-hint = Eine wartende Nachricht hat beim Neuladen ihren Anhang verloren. Sie wird nicht gesendet, bis du sie zurück in die Eingabe holst und die Datei erneut anhängst.
chat-queue-was-interjection = Während der vorherigen Antwort getippt, die vorher endete
chat-composer-interject = Zur laufenden Antwort hinzufügen
chat-composer-interject-title = Geht in die gerade entstehende Antwort (Strg/Cmd+Eingabe). Kommt beim nächsten Werkzeugschritt an; endet die Antwort vorher, wird es als nächste Nachricht gesendet.
chat-composer-interrupt = Unterbrechen und neu ansetzen
chat-composer-interrupt-title = Die laufende Antwort abbrechen und stattdessen dies senden. Das bisher Geschriebene bleibt im Gespräch.
chat-steer-pending = Während dieser Antwort ergänzt — noch nicht gelesen
chat-steer-delivered = Während dieser Antwort ergänzt — berücksichtigt
chat-steer-resent = Während dieser Antwort ergänzt — kam zu spät, als nächste Nachricht gesendet
chat-steer-discarded = Während dieser Antwort ergänzt — kam zu spät und wurde verworfen
