# STATUS: llm-generated, unreviewed — pending native-speaker QA

tokens-page-heading = API-Tokens
tokens-intro = Bearer-Tokens für die OpenAI-kompatible API. Der Klartext wird nur bei der Erstellung angezeigt — bewahren Sie ihn sicher auf.

tokens-create-heading = Token erstellen
tokens-name-label = Name
tokens-name-placeholder = z. B. laptop, ci-runner
tokens-ttl-label = TTL (Tage)
tokens-create-submit = Token erstellen

tokens-list-heading = Ihre Tokens
tokens-list-empty = Noch keine Tokens vorhanden. Erstellen Sie oben eines.

tokens-badge-revoked = widerrufen
tokens-badge-active = aktiv
tokens-remove-button = Entfernen
tokens-rotate-button = Erneuern
tokens-rotate-title = Ein neues Secret für dieses Token ausstellen (Name und Einstellungen bleiben erhalten)
tokens-revoke-button = Widerrufen

tokens-row-meta = erstellt { $created } · zuletzt verwendet { $last_used } · läuft ab { $expires }
tokens-last-used-never = nie

tokens-tool-use-aria = Werkzeugnutzung
tokens-tool-use-label = Werkzeugnutzung

tokens-mcp-allow-description = Verbindungs-Werkzeuge, die eine Bestätigung erfordern, können über die API nicht nachfragen; die Aktivierung führt sie ohne Rückfrage aus.

tokens-minted-heading = Token erstellt
tokens-minted-copy-warning = Kopieren Sie den Wert jetzt — Sie können ihn danach nicht mehr einsehen.
tokens-copy-aria = Token kopieren
tokens-copy-title = Token kopieren
tokens-minted-name = Name: { $name }

tokens-account-user-id-label = Benutzer-ID

tokens-mcp-ask-enabled-toast = „Ask“-MCP-Werkzeuge über die API für dieses Token aktiviert.
tokens-mcp-ask-disabled-toast = „Ask“-MCP-Werkzeuge über die API für dieses Token deaktiviert.

# Web Push "turn complete" opt-in card (rendered by `render_push_card`; wired
# client-side by `ui/ts/push.ts`). Device-local notification settings.
tokens-push-enable = Auf diesem Gerät aktivieren
tokens-push-disable = Auf diesem Gerät deaktivieren
tokens-push-on = Benachrichtigungen sind für dieses Gerät aktiviert.
tokens-push-enabled = Benachrichtigungen auf diesem Gerät aktiviert.
tokens-push-disabled = Benachrichtigungen auf diesem Gerät deaktiviert.
tokens-push-error = Benachrichtigungseinstellungen konnten nicht geändert werden.

# Nutzung, Modell-Freigabeliste und Kontingent pro Token (/tokens).
tokens-usage-line = diesen Monat: { $requests } Anfragen · { $tokens } Tokens · { $cost }
tokens-models-summary-all = Modelle: alle
tokens-models-summary-restricted = Modelle: { $count } ausgewählt
tokens-models-help = Ausgeschaltet folgt dieses Token Ihrem eigenen Zugriff, auch bei später hinzugefügten Modellen. Eingeschaltet darf es nur die angehakten Modelle verwenden — ein danach hinzugefügtes Modell bleibt gesperrt, bis Sie es hier ebenfalls anhaken.
tokens-models-restrict-label = Dieses Token auf bestimmte Modelle beschränken
tokens-models-save = Modelle speichern
tokens-models-saved-toast = Token auf { $count } Modelle beschränkt.
tokens-models-cleared-toast = Token darf alle Ihre Modelle verwenden.
tokens-limits-add = Kontingent hinzufügen
tokens-limits-saved-toast = Token-Kontingent gespeichert.

# SPA-only: the Svelte /tokens row panels and their confirm prompts.
tokens-models-heading = Modell-Zulassungsliste
tokens-models-input-placeholder = Modell-IDs, kommagetrennt
tokens-quota-heading = Kontingent
tokens-quota-per = pro
tokens-quota-max-placeholder = max.
tokens-mcp-heading = MCP-Konnektoren
tokens-mcp-allow-button = Ausführen zulassen
tokens-mcp-block-button = Blockieren
tokens-revoke-confirm = Dieses Token widerrufen? Clients, die es verwenden, funktionieren sofort nicht mehr.
tokens-rotate-confirm = Ein neues Secret erzeugen? Das alte funktioniert sofort nicht mehr.
tokens-remove-confirm = Diesen Token-Eintrag endgültig löschen?
