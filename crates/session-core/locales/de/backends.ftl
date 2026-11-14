# STATUS: llm-generated, unreviewed — pending native-speaker QA
# The upstream board and the backend editor: the pool band's status word, the
# editor's fields, and the connection-test outcomes.
backends-status-up = aktiv


# Backend-CRUD-Editor (Backends in der DB-Topologie hinzufügen/bearbeiten/löschen).
backends-field-name = Name
backends-field-base-url = Basis-URL
backends-field-pool = Pool
backends-field-pool-none = (keiner)
backends-field-pools = Pools
backends-field-pools-hint = Je eine Pool pro Typ, den das Konto bedient. Ein Typ tritt eine bestehende leere Pool dieses Typs bei, bekommt eine neue Pool im selben Save oder wird nicht bedient. Pools, die schon einen anderen Backend halten, werden nicht angeboten — das mischen würde diesen Backend auf diese Allowlist pinnen.
backends-pool-not-served = Nicht bedient
backends-pool-new = Neu: openrouter-{ $kind }
backends-save-backend = Backend speichern
backends-delete-backend = Löschen
backends-delete-confirm = Backend „{ $name }“ löschen? Sein Pool behält die übrigen Backends.
backends-advanced = Erweiterte Einstellungen

backends-field-api-key = API-Schlüssel
backends-field-api-key-keep = leer lassen, um den aktuellen Schlüssel zu behalten
backends-field-api-key-stored = Gespeicherter Key: { $hint }
backends-field-api-key-clear = Gespeicherten Key entfernen

# An alias that is configured but would not route.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.

# Backend-Zeilen der SPA: Wartungsschalter, Last pro Stunde und Löschbestätigung.
backends-add-heading = Backend hinzufügen
backends-name-taken = Ein Backend mit diesem Namen existiert bereits — Speichern würde es überschreiben, samt Basis-URL, Schlüssel, Modellen und Pool. Wähle einen anderen Namen, um ein zweites Backend anzulegen.
backends-field-api-key-placeholder = API-Schlüssel (verschlüsselt gespeichert)
backends-field-api-key-env = API-Schlüssel-Umgebungsvariable
backends-field-health-path = Health-Pfad
backends-field-pool-hint = Weist dieses Backend einem Pool zu. Ein Backend in mehreren Pools wird auf den hier gewählten reduziert.
backends-field-weight = Gewichtung
backends-field-max-inflight = Max. gleichzeitig
backends-field-models = Modelle (kommagetrennt; leer = über /models erkennen)
backends-aliases-managed = Aliase werden im Tab „Aliase“ bearbeitet.
backends-field-extra-headers = Zusätzliche Header (name: value pro Zeile)
backends-field-extra-headers-hint = Wird mit jeder Anfrage an dieses Backend gesendet, nach dem API-Schlüssel. Beispiel: anthropic-version: 2023-06-01, den Anthropic verlangt.
backends-field-supports-edit = Unterstützt Bildbearbeitung
backends-no-models-title = Dieses Backend meldet keine Modelle, also routet nichts dorthin und ein nackter Alias hat nichts, woran er binden könnte. Meist ein Health-Probe, der nie Daten geliefert hat.
backends-no-models = keine Modelle
backends-key-env-badge = Key: env { $var }
backends-key-env-unset-badge = env { $var } NICHT GESETZT
backends-enabled-hint = Ausschalten, um dieses Backend für Wartungsarbeiten zu leeren. Wirkt sofort. Die Modelle bleiben bekannt, also übernehmen die anderen Backends des Pools und Clients sehen einen temporären Ausfall, nie „Modell nicht gefunden“.
backends-enabled-label = Nimmt Traffic an ({ $name })
backends-activity-summary = 15m { $m15 } · 30m { $m30 } · 60m { $m60 }
backends-alias-target-title = Alias → { $target }
backends-alias-missing-title = Alias → { $target } — dieses Backend meldet dieses Modell nicht
backends-alias-disabled-title = Bare-Alias deaktiviert — dieses Backend bedient mehrere Modelle; geben Sie ihm ein explizites Ziel (Zuordnungsformular)
backends-alias-disabled-label = { $name } (deaktiviert)
backends-fallback-offline-title = fallback_offline: wird ausgeliefert, wenn jedes Backend für ein bekanntes Modell in diesem Pool ausgefallen ist
backends-fallback-offline-badge = offline ↩ { $model }
backends-pool-empty = Keine Backends in diesem Pool.

backends-test-button = Verbindung testen
backends-test-hint = Ruft diese URL mit den obigen Zugangsdaten auf. Es wird nichts gespeichert.
backends-test-models = Gemeldete Modell-Ids:
backends-test-ok = Erreichbar, authentifiziert ({ $source }), { $count } Modelle gefunden.
backends-test-ok-no-models = Erreichbar und authentifiziert ({ $source }), aber die Antwort ist weder eine OpenAI- noch eine TypeSafe-/models-Liste — die Erkennung kann sie nicht lesen. Dieses Backend kann nur die Ids bedienen, die du unter „Modelle“ einträgst.
backends-test-auth-failed = Abgelehnt mit HTTP { $status }: die Zugangsdaten wurden verweigert ({ $source }). Bis das behoben ist, bleibt die Modell-Erkennung aus und das Backend meldet nichts.
backends-test-http-error = { $url } antwortete mit HTTP { $status }.
backends-test-upstream-said = Upstream-Antwort: { $detail }
backends-test-unreachable = { $url } nicht erreichbar: { $err }
backends-test-timeout = { $url } hat nicht innerhalb von { $secs }s geantwortet.
backends-test-key-typed = mit dem oben eingetippten Schlüssel
backends-test-key-stored = mit dem gespeicherten Schlüssel
backends-test-key-env = aus env { $var }
backends-test-key-env-unset = env { $var } ist NICHT GESETZT — die Anfrage ging ohne Zugangsdaten raus
backends-test-key-none = keine Zugangsdaten gesendet
backends-error-base-url-required = Basis-URL ist erforderlich

# Was die Erkennung über ein Backend ergeben hat.
backends-parallel-mismatch = Server verarbeitet { $parallel } gleichzeitig
backends-detect-profile = erkannt: { $profile }

# OpenRouter catalog: one backend, one key, the account's models, and a
# purchase-price ceiling per selected model.
openrouter-heading = OpenRouter
openrouter-description = Eine OpenRouter-Verbindung. Wähle die Modelle, die dieses Gateway nutzen darf, und bei Bedarf die Preisgrenze pro Modell; ein Modell ohne passenden Anbieter wird nicht angeboten.
openrouter-connected = Verbunden
openrouter-key-label = Key
openrouter-credits = Guthaben
openrouter-refresh = Preise aktualisieren
openrouter-backend = Backend
openrouter-search = Modelle suchen
openrouter-select-all = Alle auswählen
openrouter-deselect-all = Alle abwählen
openrouter-selected-count = { $count } ausgewählt
openrouter-model = Modell
openrouter-models = Modelle
openrouter-filter-all = Alle
openrouter-filter-selected = Ausgewählt
openrouter-filter-fitting = Passend
openrouter-price = Listenpreis
openrouter-context = Kontext
openrouter-ceiling-prompt = Max. Eingabe $/1M
openrouter-in = In
openrouter-out = Out
openrouter-max-input = Max. Eingabe
openrouter-max-output = Max. Ausgabe
openrouter-provider-input-second = In $/Sek.
openrouter-provider-output-second = Out $/Sek.
openrouter-ceiling-prompt-second = Max. Eingabe $/Sek.
openrouter-ceiling-completion-second = Max. Ausgabe $/Sek.
openrouter-max-price-second = Max. $/Sek.
openrouter-cheapest-second = Günstigster: { $provider } mit { $prompt } / { $completion } pro Audiosekunde
openrouter-per-million = /1M
openrouter-per-second = /Sek.
openrouter-show-models = Modelle anzeigen
openrouter-hide-models = Modelle ausblenden
openrouter-ceiling-invalid = Preisgrenzen müssen leer oder eine nicht-negative Zahl sein.
openrouter-ceiling-completion = Max. Ausgabe $/1M
openrouter-strategy = Anbieter-Strategie
openrouter-strategy-cheapest = Günstigste
openrouter-strategy-fastest = Schnellste
openrouter-strategy-balanced = Ausgewogen
openrouter-min-throughput = Min. Tokens/s
openrouter-over-ceiling = Über dem Limit
openrouter-save = Auswahl speichern
openrouter-saved = Auswahl gespeichert.
openrouter-no-models = Kein Modell passt zu diesem Filter.
openrouter-error = OpenRouter: { $message }
openrouter-select-model-first = Wähle mindestens ein Modell.
openrouter-endpoints = Anbieter
openrouter-provider-input = In $/1M
openrouter-provider-output = Out $/1M
openrouter-provider-cache-read = Cache-Lesen
openrouter-provider-throughput = tok/s
openrouter-provider-latency = Latenz
openrouter-provider-uptime = Uptime
openrouter-provider-limit = Dein Limit
openrouter-usage = Verbrauch
openrouter-aria-input-ceiling = { $model } Eingabe-Limit
openrouter-aria-output-ceiling = { $model } Ausgabe-Limit
openrouter-aria-strategy = { $model } Anbieter-Strategie
openrouter-aria-min-throughput = { $model } Mindestdurchsatz
openrouter-cheapest-provider = Günstigster
openrouter-provider-fits = passt
openrouter-provider-excluded = zu teuer
openrouter-providers-fit = { $fit } von { $total } Anbietern verfügbar
openrouter-providers-hidden = { $count ->
    [one] { $count } Anbieter durch deine Grenzen ausgeblendet
   *[other] { $count } Anbieter durch deine Grenzen ausgeblendet
  }
openrouter-providers-none-fit = Kein Anbieter passt zu deinen Preisgrenzen — erhöhe oder leere Max. Eingabe / Max. Ausgabe.
openrouter-providers-fit-short = { $fit } von { $total } passen
openrouter-price-unknown = Preis ungeprüft
openrouter-price-unknown-hint = Die Anbieterpreise für dieses Modell wurden noch nicht geladen — öffne die Zeile, um sie zu sehen.
openrouter-cheapest = Günstigster: { $provider } mit { $prompt } / { $completion } pro 1M
openrouter-use-cheapest = Limit auf günstigsten setzen
openrouter-compare = Auf OpenRouter vergleichen
openrouter-provider-error = Anbieter konnten nicht geladen werden: { $message }
openrouter-provider-none = OpenRouter meldet für dieses Modell keinen Anbieter.
openrouter-provider = Anbieter
openrouter-add-heading = OpenRouter hinzufügen
openrouter-add-description = Ein Key, alles eingerichtet: der Backend plus eine Pool pro Modell-Typ, den er bedient (chat, transcription, speech, system_one, embedding, image, rerank), benannt openrouter-<typ>. Bestehende Pools mit diesem Namen werden beigetreten. Modellauswahl und Preis-Deckel folgen im OpenRouter-Panel.
openrouter-add-save = OpenRouter hinzufügen
openrouter-kinds = Modell-Typen
openrouter-kind-selected-count = { $count } ausgewählt
openrouter-kind-catalog-count = { $count } im Katalog
openrouter-kind-not-enabled = Nicht aktiviert
openrouter-key-change = Key ändern
openrouter-key-placeholder = Neuer API-Key
openrouter-key-save = Key speichern
openrouter-key-saved = Key aktualisiert
openrouter-sort = Sortierung
openrouter-save-changed = Auswahl speichern · { $count } geändert
openrouter-dirty-confirm = { $kind } hat ungespeicherte Änderungen. Verwerfen?
openrouter-provider-field = Anbieter
openrouter-provider-openai = OpenAI-kompatibel
openrouter-provider-openrouter = OpenRouter
openrouter-provider-hint = OpenRouter liest den Konto-Katalog und erzwingt das Preislimit pro Modell bei jeder Anfrage. Andere Anbieter sind reines OpenAI-kompatibles HTTP.
openrouter-max-price = Max. $/1M
openrouter-voices = Stimmen
