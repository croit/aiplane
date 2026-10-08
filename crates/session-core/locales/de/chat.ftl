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
chat-error-attachment-storage-failed = Der Anhangspeicher hat nicht korrekt geantwortet, daher konnte die Datei nicht abgerufen werden. Versuche es erneut; schlägt es weiter fehl, sollte ein Administrator den Anhangspeicher unter /admin/settings prüfen.
chat-error-turn-interrupted = Ein interner Fehler hat diese Antwort unterbrochen. Bitte versuche es erneut.

# SPA-only chat chrome (`web/src/routes/chat/*`): the conversation list,
# the conversation header, the assistant's ask-back card, and the
# turn-status line the server never renders itself.
chat-list-empty = Noch keine Unterhaltungen. Starte oben eine neue.
chat-turn-stopped = gestoppt
chat-waiting-approval = Führt { $tool } erst aus, wenn du es freigibst.
chat-waiting-value = Wartet auf einen Wert, den nur du eingeben darfst. Er geht an das Tool, nie an das Modell.
chat-waiting-answer = Wartet auf die Antwort einer Person.
chat-composer-paused = Gib oder verweigere zuerst die Freigabe oben; die nächste Nachricht wartet auf diese Entscheidung.
chat-prompt-heading = Der Assistent fragt
chat-prompt-placeholder = Antwort eingeben …
chat-prompt-answer = Antworten
chat-prompt-skip = Überspringen

# Der Eingabebereich bleibt während einer laufenden Antwort nutzbar:
# Getipptes wartet in einer Warteschlange, ein Zwischenruf geht in die
# laufende Antwort, und die Antwort lässt sich abbrechen und neu ansetzen.
chat-composer-send-during-turn-title = Senden. Während eine Antwort entsteht, wird dies ergänzt; kommt es zu spät, wird es als nächste Nachricht gesendet.
chat-composer-interrupt = Unterbrechen und neu ansetzen
chat-composer-interrupt-title = Die laufende Antwort abbrechen und stattdessen dies senden. Das bisher Geschriebene bleibt im Gespräch.
chat-turn-waiting = Gesendet — wartet auf einen freien Platz
chat-turn-waiting-cancel = Zurücknehmen
chat-steer-pending = Während dieser Antwort ergänzt — noch nicht gelesen
chat-steer-delivered = Während dieser Antwort ergänzt — berücksichtigt
chat-steer-resent = Während dieser Antwort ergänzt — kam zu spät, als nächste Nachricht gesendet
chat-steer-discarded = Während dieser Antwort ergänzt — kam zu spät und wurde verworfen

linked-chat-label = Fortsetzen in
linked-chat-fresh = Einem neuen Chat, den der nächste Lauf öffnet
linked-chat-help = Jeder Lauf hängt seinen Prompt und die Antwort an diesen Chat an. Wähle eine deiner Unterhaltungen, um alles in einem Verlauf zu halten.

# A model call that looped and was retried at a lower thinking level, shown
# collapsed above the answer. $effort is a level name (off/low/medium/high/xhigh).
chat-attempt-summary = Versuch { $n } abgebrochen: { $reason } (Denkaufwand: { $effort })
chat-attempt-reason-loop = das Modell hat sich wiederholt
chat-attempt-reason-repeated_call = immer wieder derselbe Tool-Aufruf
chat-attempt-retry = Neuer Versuch mit Denkaufwand { $effort } (Versuch { $n })
chat-loop-exhausted = Das Modell hat sich bei allen { $attempts } Versuchen wiederholt, auch mit weniger Denkaufwand. Formuliere die Frage bitte anders oder teile sie auf.
