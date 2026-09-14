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
render-composer-record-aria = Sprachnachricht aufnehmen
render-composer-record-title = Aufnehmen
render-composer-send = Senden
render-composer-stop = Stopp

# Shown over the whole conversation while a file is dragged across it, and
# the refusal when the drop turned out to be a folder.
render-drop-overlay = Dateien hier ablegen, um sie anzuhängen
render-drop-overlay-hint = Die Dateien werden an die nächste Nachricht angehängt
render-drop-folders-unsupported = Ordner können nicht angehängt werden — lege stattdessen die Dateien darin ab.

# The browser prompt behind the ✎ Edit button, and the per-file title on
# an attachment's remove button.
render-edit-prompt = Deine Nachricht bearbeiten:
render-attachment-remove-title = { $filename } entfernen
render-edit-confirm = Speichern und neu generieren? Dadurch werden alle Nachrichten darunter gelöscht.
render-edit-save = Speichern & neu generieren
render-edit-cancel = Abbrechen
render-retry-confirm = Diese Antwort neu generieren? Dadurch werden sie und alles darunter gelöscht.
render-attachment-remove-confirm = { $filename } entfernen? Das kann nicht rückgängig gemacht werden.
render-attachment-unavailable-title = Dieser Anhang ist nicht mehr verfügbar
render-attachment-unavailable-meta = nicht verfügbar
render-attachment-open-title = { $filename } öffnen · { $mime } · { $size }
render-attachment-title = { $filename } · { $mime } · { $size }
render-media-label = { $kind ->
    [image] Bild { $n }
    [video] Video { $n }
    [audio] Audio { $n }
   *[other] Medium { $n }
}
render-code-copy = Code kopieren
render-code-copied = Kopiert
render-still-working-spinner = Arbeitet noch…
render-thinking-in-progress = Denkt nach… ({ $secs } s)
render-tools-running = Tools laufen
render-tools-errored = Tool-Aufrufe
render-tools-used = Verwendete Tools
render-tools-summary = { $count } Aufrufe · { $breakdown }
render-tool-status-calling = Wird aufgerufen
render-tool-status-error = Tool-Fehler
render-tool-input-label = Eingabe
render-tool-output-label = Ausgabe
render-tool-output-truncated = für die Anzeige gekürzt — alle { $bytes } Bytes sind weiterhin für das Modell verfügbar und in der Datenbank gespeichert; die ersten { $chars } Zeichen werden angezeigt
render-compaction-divider = Frühere Nachrichten zur Kontexteinsparung zusammengefasst
render-canvas-version-by-you = von dir
render-canvas-hand-edited = von dir bearbeitet
render-canvas-edit-hint = Wird als neue Version gespeichert; der Assistent erfährt von deiner Änderung.
render-canvas-version-aria = Version
render-canvas-resize-aria = Zeichenfläche skalieren
