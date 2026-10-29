# STATUS: llm-generated, unreviewed — pending native-speaker QA

agent-output-withheld = Diese Antwort kann ich nicht geben, weil sie Angaben enthielt, die ich für Sie nicht bestätigen konnte. Bitte formulieren Sie Ihre Frage um oder wenden Sie sich an den Support.
agent-output-redacted = [entfernt]
agent-error-name-taken = Ein persönlicher Agent mit diesem Namen existiert bereits in Ihrem Konto. Wählen Sie einen anderen Namen.
agent-error-name-invalid = Geben Sie einen Namen mit höchstens 80 Zeichen ohne Steuerzeichen ein.
agent-error-active-limit = Das Limit für aktive persönliche Agenten ist erreicht. Deaktivieren Sie einen Agenten, bevor Sie einen weiteren erstellen.
agent-error-not-found = Dieser Agent ist für Sie nicht verfügbar.
agent-error-id-missing = In der Anfrage fehlt eine Agenten-ID.
agent-memory-error-kind = Wählen Sie als Erinnerungsart Präferenz, Projekt oder Fakt.
agent-memory-error-empty = Der Inhalt der Erinnerung darf nicht leer sein.
agent-memory-error-id-missing = In der Anfrage fehlt eine Agenten- oder Erinnerungs-ID.

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
agent-error-inactive = Dieser Agent ist deaktiviert. Aktivieren Sie ihn, bevor Sie eine Unterhaltung starten.
agent-action-chat = Chat
agent-action-chat-opening = Chat wird geöffnet …
agent-activity-session-started = Agenten-Chat gestartet.

agent-workspace-error-unavailable = Der dauerhafte Agenten-Speicher ist nicht verfügbar. Bitte die Administration, den Sandbox-Runner zu prüfen.
agent-workspace-error-conflict = Der Arbeitsbereich ist belegt oder wurde geändert. Lade ihn neu und versuche es erneut, wenn der Agent nicht arbeitet.
agent-workspace-error-not-found = Dieser Eintrag wurde nicht gefunden. Lade das Verzeichnis neu.
agent-workspace-error-too-large = Die Datei oder das Verzeichnis überschreitet das Speicherlimit. Verwende eine kleinere Datei oder ein kleineres Verzeichnis.
agent-workspace-error-invalid = Die Anfrage ist ungültig. Prüfe den Pfad und versuche es erneut.
agent-workspace-error-failed = Die Aktion im Arbeitsbereich ist fehlgeschlagen. Lade ihn neu und versuche es erneut.

agent-workspace-heading = Arbeitsbereich
agent-workspace-intro = Dateien in diesem Arbeitsbereich bleiben über Agenten-Ausführungen hinweg erhalten. Lade Dateien herunter, um sie anzusehen.
agent-workspace-reload = Arbeitsbereich neu laden
agent-workspace-unavailable = Dauerhafter Speicher ist nicht verfügbar. Bitten Sie Ihre Administration, den Sandbox-Runner zu prüfen.
agent-workspace-busy = Dieser Agent verwendet seinen Arbeitsbereich. Dateien können sich ändern. Hochladen, Ordner erstellen und Löschen sind möglich, sobald er nicht mehr arbeitet.
agent-workspace-empty = Dieser Ordner ist leer.
agent-workspace-root = Hauptverzeichnis des Arbeitsbereichs
agent-workspace-folder = Ordner
agent-workspace-size-missing = Größe nicht verfügbar
agent-workspace-modified = Geändert { $when }
agent-workspace-upload = Datei hochladen
agent-workspace-file = Datei
agent-workspace-upload-limit = Wählen Sie eine Datei mit höchstens 50 MiB.
agent-workspace-create-folder = Ordner erstellen
agent-workspace-folder-name = Ordnername
agent-workspace-delete = Löschen
agent-workspace-delete-confirm = „{ $name }“ löschen? Ordner müssen leer sein.
agent-workspace-more = Weitere Dateien laden
agent-workspace-error-load = Der Arbeitsbereich konnte nicht geladen werden. Lade ihn neu und versuche es erneut.
agent-workspace-error-save = Die Änderung konnte nicht gespeichert werden. Lade den Arbeitsbereich neu und versuche es erneut.
agent-workspace-notice-saved = Arbeitsbereich aktualisiert.
