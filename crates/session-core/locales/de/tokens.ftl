# STATUS: llm-generated, unreviewed — pending native-speaker QA

tokens-page-heading = API-Tokens
tokens-intro = Tokens für Apps und Code, die sich mit AIplane verbinden. Der vollständige Wert wird nur beim Erstellen oder Erneuern angezeigt — kopieren und geheim halten.

tokens-create-heading = Token erstellen
tokens-name-label = Name
tokens-name-placeholder = z. B. laptop, ci-runner
tokens-ttl-label = TTL (Tage)
tokens-create-submit = Token erstellen

tokens-list-heading = Ihre Tokens
tokens-list-empty = Noch keine Tokens vorhanden. Erstellen Sie eines über die Schaltfläche oben.

tokens-badge-revoked = widerrufen
tokens-badge-active = aktiv
tokens-remove-button = Entfernen
tokens-rotate-button = Erneuern
tokens-rotate-title = Ein neues Secret für dieses Token ausstellen (Name und Einstellungen bleiben erhalten)
tokens-revoke-button = Widerrufen

tokens-row-meta = erstellt { $created } · zuletzt verwendet { $last_used } · läuft ab { $expires }
tokens-last-used-never = nie

tokens-tool-use-label = Werkzeugnutzung

tokens-mcp-allow-description = Verbindungs-Werkzeuge, die eine Bestätigung erfordern, können über die API nicht nachfragen; die Aktivierung führt sie ohne Rückfrage aus.

tokens-minted-heading = Token erstellt
tokens-minted-copy-warning = Kopieren Sie den Wert jetzt — Sie können ihn danach nicht mehr einsehen.
tokens-copy-aria = Token kopieren
tokens-minted-name = Name: { $name }

tokens-account-user-id-label = Benutzer-ID

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
tokens-models-summary-restricted = Modelle: { $count } ausgewählt
tokens-models-help = Ausgeschaltet folgt dieses Token Ihrem eigenen Zugriff, auch bei später hinzugefügten Modellen. Eingeschaltet darf es nur die angehakten Modelle verwenden — ein danach hinzugefügtes Modell bleibt gesperrt, bis Sie es hier ebenfalls anhaken.
tokens-models-restrict-label = Dieses Token auf bestimmte Modelle beschränken
tokens-models-saved-toast = Token auf { $count } Modelle beschränkt.
tokens-models-cleared-toast = Token darf alle Ihre Modelle verwenden.
tokens-limits-add = Kontingent hinzufügen
tokens-limits-saved-toast = Token-Kontingent gespeichert.

# SPA-only: the Svelte /tokens row panels and their confirm prompts.
tokens-quota-max-placeholder = max.
tokens-revoke-confirm = Dieses Token widerrufen? Clients, die es verwenden, funktionieren sofort nicht mehr.
tokens-rotate-confirm = Ein neues Secret erzeugen? Das alte funktioniert sofort nicht mehr.
tokens-remove-confirm = Diesen Token-Eintrag endgültig löschen?

tokens-create-description = Erstellen Sie einen neuen Bearer-Token für die OpenAI-kompatible API.
tokens-tool-use-description = Erlaubt diesem Token, Gateway-Werkzeuge (Websuche, RAG, …) aufzurufen.
tokens-capabilities-summary = Fähigkeiten
tokens-panel-close = Schließen
tokens-edit-button = Bearbeiten
tokens-mcp-allow-label = „Ask“-MCP-Werkzeuge über die API erlauben
tokens-account-heading = Konto
tokens-signed-in-as = Angemeldet als { $email }
tokens-account-oidc-label = OIDC-Rollen
tokens-account-rbac-label = RBAC-Rollen-IDs
tokens-roles-none = keine
tokens-roles-none-granted = keine vergeben
tokens-push-heading = Benachrichtigungen
tokens-push-description = Erhalten Sie auf diesem Gerät eine Benachrichtigung, wenn eine von Ihnen gestartete Antwort fertig ist, während Sie nicht in der App sind.
tokens-push-off = Benachrichtigungen sind für dieses Gerät deaktiviert.
tokens-push-denied = Dieser Browser hat Benachrichtigungen blockiert. Erlauben Sie sie in den Browsereinstellungen, um sie zu aktivieren.
tokens-push-unsupported = Dieser Browser unterstützt keine Benachrichtigungen.
tokens-models-none-picked = Haken Sie mindestens ein Modell an oder schalten Sie die Beschränkung aus.
tokens-limits-help = Eine Obergrenze allein für dieses Token. Ihr eigenes Budget gilt weiterhin — das hier kann den Verbrauch nur enger fassen, nie erweitern.
tokens-limits-remove = Entfernen
tokens-limits-admin-badge = vom Administrator gesetzt
tokens-models-admin-set = Ein Betreiber beschränkt dieses Token zusätzlich auf: { $models }. Ihre eigene Auswahl kann das nur weiter einschränken, nicht erweitern.

# Client setup guides and token-page tabs.
tokens-tab-tokens = Tokens
tokens-tab-guides = Einrichtungsanleitungen
tokens-tab-account = Konto
tokens-guides-heading = Client einrichten
tokens-guides-intro = Wählen Sie unten eine App und folgen Sie den Schritten. Ein Token kann in mehreren Apps verwendet werden; ein eigenes Token pro App ist leichter zu verwalten.
tokens-guides-before = Erstellen Sie zuerst im Tab „Tokens“ ein Token und kopieren Sie es. Der vollständige Wert wird nur einmal angezeigt. Halten Sie ihn geheim.
tokens-guide-opencode = OpenCode
tokens-guide-claude = Claude Code
tokens-guide-python = Python (OpenAI)
tokens-opencode-step-1 = Erstellen Sie im Tab „Tokens“ ein Token und kopieren Sie es.
tokens-opencode-step-2 = Führen Sie in OpenCode /connect aus, wählen Sie Other, geben Sie aiplane als Provider-ID ein und fügen Sie Ihr Token ein.
tokens-opencode-step-3 = Speichern Sie diese Konfiguration in ~/.config/opencode/opencode.jsonc. Ersetzen Sie YOUR_MODEL_ID durch eine nutzbare Chat-Modell-ID.
tokens-opencode-finish = Starten Sie opencode und wählen Sie mit /models aiplane/YOUR_MODEL_ID.
tokens-claude-step-1 = Erstellen Sie im Tab „Tokens“ ein Token und kopieren Sie es.
tokens-claude-step-2 = Wählen Sie eine nutzbare Chat-Modell-ID. Ihr Administrator hat möglicherweise einen Alias für Claude Code eingerichtet.
tokens-claude-step-3 = Fügen Sie diese Befehle in Ihr Terminal ein. Ersetzen Sie vorher Token und Modell-ID.
tokens-claude-finish = Claude Code verwendet die Basis-URL ohne /v1. Fragen Sie bei einem Fehler „Modell nicht gefunden“ Ihren Administrator nach dem passenden Alias.
tokens-python-step-1 = Erstellen Sie im Tab „Tokens“ ein Token und kopieren Sie es.
tokens-python-step-2 = Installieren Sie das OpenAI-Python-Paket mit python -m pip install openai und setzen Sie OPENAI_API_KEY im Terminal auf Ihr Token.
tokens-python-step-3 = Speichern Sie das Beispiel als chat.py. Ersetzen Sie YOUR_MODEL_ID durch eine nutzbare Chat-Modell-ID.
tokens-python-finish = Führen Sie python chat.py aus. Speichern Sie das Token in einer Umgebungsvariable, nicht im Skript.
tokens-guide-omp = Oh My Pi
tokens-guide-pi = Pi
tokens-omp-finish = Starten Sie omp und wählen Sie mit /model aiplane/YOUR_MODEL_ID.
tokens-omp-step-1 = Erstellen Sie im Tab „Tokens“ ein Token und kopieren Sie es.
tokens-omp-step-2 = Speichern Sie diese Konfiguration in ~/.omp/agent/models.yml. Ersetzen Sie zuerst das Token und YOUR_MODEL_ID und setzen Sie contextWindow auf die Kontextgröße des Modells.
tokens-pi-finish = Starten Sie pi und wählen Sie mit /model aiplane/YOUR_MODEL_ID.
tokens-pi-step-1 = Erstellen Sie im Tab „Tokens“ ein Token und kopieren Sie es.
tokens-pi-step-2 = Speichern Sie diese Konfiguration in ~/.pi/agent/models.json. Ersetzen Sie zuerst das Token und YOUR_MODEL_ID.
tokens-guides-model-note = Sie brauchen eine Modell-ID? Verfügbare Modelle stehen in der Modellauswahl des Chats oder unter GET /v1/models mit Ihrem Token.

notifications-loading = Benachrichtigungseinstellungen werden geladen …
notifications-unavailable = Benachrichtigungen sind auf diesem Gateway derzeit nicht verfügbar. Bitten Sie eine Administratorin oder einen Administrator, die Push-Einstellungen zu prüfen.
notifications-admin-settings-link = Push-Einstellungen öffnen
tokens-tile-models = Modelle
tokens-tile-tools = Werkzeuge
tokens-tile-budget = Budget
tokens-save = Speichern
tokens-menu-aria = Token-Aktionen
tokens-expires-today = läuft heute ab
tokens-expires-soon = { $days ->
    [one] läuft in { $days } Tag ab
   *[other] läuft in { $days } Tagen ab
}
tokens-models-tile-all = Alle { $count } Modelle – folgt Ihrem Zugriff
tokens-models-tile-some = { $count } von { $total } Modellen
tokens-models-tile-compliant = Alle GDPR-konform und NDA-gedeckt
tokens-models-tile-noncompliant = { $count ->
    [one] { $count } Modell ohne GDPR- oder NDA-Schutz
   *[other] { $count } Modelle ohne GDPR- oder NDA-Schutz
}
tokens-models-tile-admin = Ein Operator erlaubt davon { $count }
tokens-models-max-price = bis { $price } pro 1 Mio. Ausgabe-Tokens
tokens-models-noncompliant-warning = { $count ->
    [one] { $count } Modell hier ist nicht GDPR-konform oder nicht NDA-gedeckt. Senden Sie über diesen Token keine personenbezogenen Daten oder vertraulichen Inhalte.
   *[other] { $count } Modelle hier sind nicht GDPR-konform oder nicht NDA-gedeckt. Senden Sie über diesen Token keine personenbezogenen Daten oder vertraulichen Inhalte.
}
tokens-models-search = Modelle suchen
tokens-models-filter-gdpr = GDPR-konform
tokens-models-filter-nda = NDA-gedeckt
tokens-models-filter-free = Kostenlos
tokens-models-select-compliant = Alle mit GDPR + NDA auswählen
tokens-models-select-none = Auswahl leeren
tokens-models-empty = Kein Modell passt.
tokens-models-alias = Alias für { $target }
tokens-models-gdpr-ok = GDPR-konform: personenbezogene Daten bleiben geschützt.
tokens-models-nda-ok = Durch eine Vertraulichkeitsvereinbarung gedeckt.
tokens-models-price-free = kostenlos
tokens-models-price-tokens = { $input } ein / { $output } aus je 1 Mio.
tokens-models-price-per-images = { $price } pro Bild
tokens-models-price-per-characters = { $price } pro Zeichen
tokens-models-price-per-seconds = { $price } pro Sekunde
tokens-models-kind-chat = Chat
tokens-models-kind-transcription = Sprache zu Text
tokens-models-kind-speech = Text zu Sprache
tokens-models-kind-embedding = Embedding
tokens-models-kind-rerank = Reranking
tokens-models-kind-image = Bilder
tokens-models-kind-system_one = System One
tokens-tools-tile-on = Erlaubt
tokens-tools-tile-off = Aus
tokens-tools-tile-pinned = { $count } immer an
tokens-tools-tile-mcp-allowed = „Ask“-MCP-Werkzeuge laufen ohne Rückfrage
tokens-tools-tile-mcp-blocked = „Ask“-MCP-Werkzeuge gesperrt
tokens-tools-capabilities-help = Welche Werkzeuge und Skills dieser Token nutzen darf.
tokens-tools-saved-toast = Werkzeuge des Tokens gespeichert.
tokens-budget-tile-none = Kein eigenes Limit
tokens-budget-tile-more = +{ $count } weitere
tokens-budget-owner-applies = Ihr eigenes Budget gilt zusätzlich
tokens-budget-token-heading = Dieser Token
tokens-budget-owner-heading = Gilt zusätzlich: Ihr eigenes Budget
tokens-budget-owner-none = Für Sie gilt kein Budgetlimit.
tokens-budget-dimension = Was begrenzt wird
tokens-budget-window = Zeitraum
