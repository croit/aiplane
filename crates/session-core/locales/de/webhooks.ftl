# STATUS: llm-generated, unreviewed — pending native-speaker QA

webhooks-heading = Webhooks
webhooks-intro = Führe einen Prompt aus, wenn ein externer Dienst eine URL aufruft. Du erhältst eine geheime Trigger-URL; der Inhalt, den der Aufrufer im Anfrage-Body sendet, wird an deinen Prompt angehängt, und der Lauf öffnet sich als neuer Chat, den du hier lesen kannst.
webhooks-edit-heading = Webhook bearbeiten
webhooks-list-empty = Noch keine Webhooks. Erstelle oben einen.

webhooks-name-label = Name
webhooks-name-placeholder = z. B. Deploy-Zusammenfassung
webhooks-model-label = Modell
webhooks-model-placeholder = Modell-ID
webhooks-prompt-placeholder = Was soll das Modell mit den eingehenden Daten tun?

webhooks-reveal-heading = Deine Trigger-URL
webhooks-reveal-note = Kopiere sie jetzt — sie wird nur einmal angezeigt. Jeder mit dieser URL kann den Webhook auslösen. Verloren? Erzeuge über „Rotieren" eine neue.
webhooks-copy = Kopieren

webhooks-badge-active = Aktiv
webhooks-badge-paused = Pausiert
webhooks-mode-sync = Wartet auf Antwort

webhooks-pause-title = Pausieren
webhooks-resume-title = Fortsetzen
webhooks-rotate-title = Secret rotieren
webhooks-edit-title = Bearbeiten
webhooks-delete-title = Löschen

# --- Mit anderem Prompt erneut ausführen ---
webhooks-toast-rerun-started = Erneute Ausführung abgeschlossen — Konversation wird geöffnet…

# --- Ausführungshistorie ---
webhooks-runs-empty = Noch keine Ausführungen. Löse den Webhook aus, um hier die Historie zu sehen.
webhooks-run-open = Chat öffnen
webhooks-run-rerun = erneut ausführen

# SPA-only: the Svelte /webhooks page — inline form, run list, rerun composer.
webhooks-new-heading = Neuer Webhook
webhooks-prompt-untrusted-label = Prompt (die Nutzlast kommt als nicht vertrauenswürdige Eingabe an)
webhooks-runs-show = Läufe
webhooks-runs-hide = Läufe ausblenden
webhooks-rerun-prompt-label = Prompt für den erneuten Lauf — die gespeicherte Nutzlast wird damit erneut verarbeitet
webhooks-rerun-latest = Letzte Nutzlast erneut ausführen
webhooks-rerun-running = Läuft …
webhooks-toast-rerun-failed = Erneuter Lauf: { $status }
webhooks-rotate-confirm = Ein neues Trigger-Secret erzeugen? Die alte URL funktioniert sofort nicht mehr.
webhooks-delete-confirm = Diesen Webhook löschen?
