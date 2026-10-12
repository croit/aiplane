# STATUS: llm-generated, unreviewed — pending native-speaker QA

agent-output-withheld = Diese Antwort kann ich nicht geben, weil sie Angaben enthielt, die ich für Sie nicht bestätigen konnte. Bitte formulieren Sie Ihre Frage um oder wenden Sie sich an den Support.
agent-output-redacted = [entfernt]
dot-error-name-taken = Ein persönlicher Dot mit diesem Namen existiert bereits in Ihrem Konto. Wählen Sie einen anderen Namen.
dot-error-name-invalid = Geben Sie einen Namen mit höchstens 80 Zeichen ohne Steuerzeichen ein.
dot-error-active-limit = Das Limit für aktive persönliche Dots ist erreicht. Deaktivieren Sie einen Dot, bevor Sie einen weiteren erstellen.
dot-error-not-found = Dieser Dot ist für Sie nicht verfügbar.
dot-error-id-missing = In der Anfrage fehlt eine Dot-ID.
dot-memory-error-kind = Wählen Sie als Erinnerungsart Präferenz, Projekt oder Fakt.
dot-memory-error-empty = Der Inhalt der Erinnerung darf nicht leer sein.
dot-memory-error-id-missing = In der Anfrage fehlt eine Dot- oder Erinnerungs-ID.

# What a website visitor reads next to the secure field when an identity
# verifier (#95) asks for the code it sent. Worded the same whether or not
# the address is registered, so it reveals neither.
agent-verifier-code-sent = Wenn diese Adresse bei uns registriert ist, haben wir einen Code dorthin gesendet. Geben Sie den Code hier ein.
agent-verifier-code-again = Bitte geben Sie den Code ein, den wir Ihnen gesendet haben.

# What a website visitor reads when the public agent endpoint refuses a
# request (#92): the agent's budget is spent, or a rate limit was hit.
agent-embed-unavailable = Dieser Assistent ist vorübergehend nicht verfügbar. Bitte versuchen Sie es später erneut oder nutzen Sie die anderen Kontaktmöglichkeiten der Website.
agent-embed-rate-limited = Sie senden Nachrichten schneller, als dieser Assistent sie annimmt. Bitte warten Sie { $seconds } Sekunden und versuchen Sie es dann erneut.

agent-embed-decision-for-staff = Diese Anfrage wartet auf eine Entscheidung unseres Teams. Sobald entschieden ist, sehen Sie die Antwort hier.
agent-embed-not-waiting = Der Assistent wartet gerade nicht auf eine Antwort von Ihnen. Bitte laden Sie das Gespräch neu.
agent-embed-message-waiting = Ihre vorherige Nachricht wartet noch auf den Assistenten. Bitte warten Sie seine Antwort ab, bevor Sie eine weitere senden.

agent-a2a-input-required = Ein Partnerdienst benötigt noch eine Angabe, um fortzufahren. Geben Sie sie hier ein; sie geht nur an diesen Dienst.
dot-error-inactive = Dieser Dot ist deaktiviert. Aktivieren Sie ihn, bevor Sie eine Unterhaltung starten.
dot-action-chat = Chat
dot-action-chat-opening = Chat wird geöffnet …
dot-activity-session-started = Dot-Chat gestartet.

dot-workspace-error-unavailable = Der dauerhafte Dot-Speicher ist nicht verfügbar. Bitte die Administration, den Sandbox-Runner zu prüfen.
dot-workspace-error-conflict = Der Arbeitsbereich ist belegt oder wurde geändert. Lade ihn neu und versuche es erneut, wenn der Dot nicht arbeitet.
dot-workspace-error-not-found = Dieser Eintrag wurde nicht gefunden. Lade das Verzeichnis neu.
dot-workspace-error-too-large = Die Datei oder das Verzeichnis überschreitet das Speicherlimit. Verwende eine kleinere Datei oder ein kleineres Verzeichnis.
dot-workspace-error-invalid = Die Anfrage ist ungültig. Prüfe den Pfad und versuche es erneut.
dot-workspace-error-failed = Die Aktion im Arbeitsbereich ist fehlgeschlagen. Lade ihn neu und versuche es erneut.

dot-workspace-heading = Arbeitsbereich
dot-workspace-intro = Dateien in diesem Arbeitsbereich bleiben über Dot-Ausführungen hinweg erhalten. Lade Dateien herunter, um sie anzusehen.
dot-workspace-reload = Arbeitsbereich neu laden
dot-workspace-unavailable = Dauerhafter Speicher ist nicht verfügbar. Bitte deine Administration, den Sandbox-Runner zu prüfen.
dot-workspace-busy = Dieser Dot verwendet seinen Arbeitsbereich. Dateien können sich ändern. Hochladen, Ordner erstellen und Löschen sind möglich, sobald er nicht mehr arbeitet.
dot-workspace-empty = Dieser Ordner ist leer.
dot-workspace-root = Hauptverzeichnis des Arbeitsbereichs
dot-workspace-folder = Ordner
dot-workspace-size-missing = Größe nicht verfügbar
dot-workspace-modified = Geändert { $when }
dot-workspace-upload = Datei hochladen
dot-workspace-file = Datei
dot-workspace-upload-limit = Wähle eine Datei mit höchstens 50 MiB.
dot-workspace-create-folder = Ordner erstellen
dot-workspace-folder-name = Ordnername
dot-workspace-delete = Löschen
dot-workspace-delete-confirm = „{ $name }“ löschen? Ordner müssen leer sein.
dot-workspace-more = Weitere Dateien laden
dot-workspace-error-load = Der Arbeitsbereich konnte nicht geladen werden. Lade ihn neu und versuche es erneut.
dot-workspace-error-save = Die Änderung konnte nicht gespeichert werden. Lade den Arbeitsbereich neu und versuche es erneut.
dot-workspace-notice-saved = Arbeitsbereich aktualisiert.
