# Strings owned by `gateway/src/rama_server/pages/admin.rs` — die Seite
# `/admin/models`: Standardmodell-Auswahl plus eine filterbare Liste aller
# angebotenen Modelle mit einem einzigen konsolidierten Editor (Preise,
# Kontextfenster, Reasoning-Stil + Budgets/Aufwand, Fähigkeiten, Sampling).

admin-heading = Modelle

# Spaltenüberschriften der Liste.
admin-col-model = Modell

# Werte in eingeklappten Zeilen.
admin-not-configured = nicht konfiguriert

# „Konfiguriert“-Facetten-Badges.
admin-badge-ctx = KTX

# Editor.
admin-save-model = Modell speichern
admin-edit-model = Bearbeiten
admin-edit-model-page-title = Modell bearbeiten
admin-model-not-found = Kein solches Modell. Möglicherweise wird es von keinem Backend mehr angeboten.
admin-clear-overrides = Alle Overrides löschen
admin-cancel = Abbrechen

admin-toml-defaults-label = Sampling-Standardwerte (TOML)

# Preise pro Modell für die Kostenabrechnung (Preis pro 1 Mio. Tokens, Eingabe / Ausgabe).
admin-price-in-label = Preis ein
admin-price-out-label = Preis aus
admin-price-in-placeholder = kein Preis
admin-price-out-placeholder = kein Preis

# Kontextfenster (steuert die Auto-Kompaktierung).
admin-context-window-full-label = Kontextfenster (Tokens)
admin-context-window-placeholder = Standard

# Standardmodelle pro Funktion (im Chat/Sprach-Auswahlmenü vorausgewählt und
# als API-Fallback, wenn ein Aufruf kein Modell angibt).
admin-defaults-heading = Standardmodelle

# Modell-Fähigkeiten (tri-state) + Fallback-Modelle.
admin-cap-tools = Tools

# Websuche-Backend (Tool `search_web`). Früher die Umgebungsvariablen
# SEARCH_PROVIDER / SEARXNG_URL / BRAVE_SEARCH_API_KEY.
admin-search-heading = Websuche
admin-search-provider-label = Anbieter
admin-search-provider-searxng = SearXNG (selbst gehostet)
admin-search-provider-brave = Brave Search API
admin-search-searxng-url-label = SearXNG-Basis-URL
admin-search-searxng-url-placeholder = https://searxng.example.com
admin-search-brave-key-label = Brave-API-Key
admin-search-brave-key-placeholder = leer lassen, um den aktuellen Key zu behalten
admin-search-save = Websuche speichern
admin-search-saved = Websuche-Einstellungen gespeichert

# ─── SvelteKit-Admin-SPA ─────────────────────────────────────────────────────
# Die Rollenprüfung der Admin-Hülle, der Workflow-Katalog unter /admin/comfyui
# und die Teile des Modell-Editors, die es auf der alten Seite nicht gab.

admin-needs-admin-role = Diese Seiten erfordern die Administratorrolle.
admin-overwrite-existing = vorhandenen Eintrag überschreiben
admin-comfyui-reload = Katalog neu laden
admin-comfyui-reloaded = Katalog neu geladen — { $count } Workflow(s) geladen.
admin-comfyui-empty = Keine Workflows geladen — prüfe das Inhaltsverzeichnis.
admin-comfyui-heading = ComfyUI-Workflow-Katalog
admin-comfyui-page-title = ComfyUI — Workflow-Katalog
admin-comfyui-intro = Headless-ComfyUI-Worker. Das Gateway stellt jeden geladenen Workflow als aufrufbares comfyui_<id>-Tool bereit. Benutzer sehen ComfyUI selbst nie.
admin-comfyui-not-configured = Nicht konfiguriert
admin-comfyui-not-configured-help = ComfyUI unter Einstellungen aktivieren, Basis-URL und Workflow-Verzeichnis festlegen und das Gateway neu starten.
admin-comfyui-operator-config = Betreiberkonfiguration
admin-comfyui-worker-url = Worker-Basis-URL
admin-comfyui-content-directory = Inhaltsverzeichnis
admin-comfyui-timeout = Workflow-Zeitlimit
admin-comfyui-poll-interval = Abfrageintervall der Warteschlange
admin-comfyui-config-help = Das Inhaltsverzeichnis wird vom Betreiber verwaltet und gehört nicht zum öffentlichen Repository. Manifeste dort bearbeiten und oben Neu laden klicken — kein Neustart nötig.
admin-comfyui-loaded-workflows = Geladene Workflows
admin-comfyui-node = Knoten { $id }
admin-comfyui-parameters = Parameter
admin-comfyui-required = erforderlich
admin-comfyui-recent-jobs = Letzte Aufträge
admin-comfyui-reloaded-skipped = Katalog neu geladen — { $count } Workflow(s) geladen, { $skipped } übersprungen.
admin-comfyui-max-concurrent = Gleichzeitige Aufträge
admin-comfyui-tab-workflows = Workflows
admin-comfyui-tab-jobs = Aufträge
admin-comfyui-jobs-page-title = ComfyUI — Aufträge
admin-comfyui-jobs-intro = Alle kürzlich von den Modellen ausgeführten Workflows, neueste zuerst — was erzeugt wurde, wie lange es gedauert hat und aus welcher Unterhaltung der Auftrag stammt.
admin-comfyui-jobs-window = Die { $count } zuletzt aufgezeichneten Läufe.
admin-comfyui-jobs-empty = Noch keine Läufe aufgezeichnet.
admin-comfyui-worker-status = Worker
admin-comfyui-worker-reachable = Erreichbar
admin-comfyui-worker-unreachable = Nicht erreichbar
admin-comfyui-worker-checking = Wird geprüft…
admin-comfyui-worker-queue = { $running } laufend · { $pending } in Warteschlange
admin-comfyui-worker-software = ComfyUI { $version } · Python { $python } · PyTorch { $torch }
admin-comfyui-worker-vram = { $free } frei von { $total }
admin-comfyui-search-placeholder = Workflows und Parameter durchsuchen
admin-comfyui-search-empty = Kein Workflow passt zu dieser Suche.
admin-comfyui-filename-prefix = Ausgabepräfix
admin-comfyui-required-count = { $required } von { $total } erforderlich
admin-comfyui-no-params = Dieser Workflow hat keine Parameter.
admin-comfyui-detail-empty = Workflow auswählen, um den Vertrag zu sehen, den das Modell erhält.
admin-comfyui-param-column = Parameter
admin-comfyui-param-description-column = Beschreibung
admin-comfyui-filter-all = Alle
admin-comfyui-filter-completed = Abgeschlossen
admin-comfyui-filter-pending = Ausstehend
admin-comfyui-filter-failed = Fehlgeschlagen
admin-comfyui-stats-heading = Zuverlässigkeit je Workflow
admin-comfyui-col-workflow = Workflow
admin-comfyui-col-runs = Läufe
admin-comfyui-col-failed = Fehlgeschlagen
admin-comfyui-col-median = Median
admin-comfyui-col-status = Status
admin-comfyui-col-duration = Dauer
admin-comfyui-col-when = Gestartet
admin-comfyui-col-result = Ergebnis
admin-comfyui-job-open = Unterhaltung öffnen
admin-comfyui-job-still-running = läuft noch
admin-comfyui-refresh = Aktualisieren
admin-clear-overrides-confirm = Alle gespeicherten Überschreibungen für { $model } verwerfen?
admin-cap-no-fallback = (keiner)

admin-page-title = Modelle — AIplane

admin-intro-prefix = Einstellungen pro Modell — Preise, Kontextfenster, Reasoning, Fähigkeiten und Sampling-Standardwerte — angewendet auf

admin-intro-every = jede

admin-intro-middle = Anfrage für dieses Modell, von jedem Benutzer oder Token, es sei denn, der Aufrufer setzt denselben Wert, was

admin-intro-always-wins = immer gewinnt

admin-intro-suffix = . Chat-Modelle, Aliase und andere Arten sind alle in einer Liste.

admin-no-models = Noch keine Modelle verfügbar. Sobald ein Upstream-Backend erreichbar ist, erscheint es hier.

admin-filter-placeholder = Modelle filtern…

admin-filter-all = Alle

admin-filter-chat = chat

admin-filter-other = andere Arten

admin-filter-aliases = Aliase

admin-filter-configured = nur konfigurierte

admin-col-kind = Art

admin-col-price = Preis ein/aus

admin-col-context = Kontext

admin-col-reasoning = Reasoning

admin-col-configured = Konfiguriert

admin-value-default = Standard

admin-value-na = n/v

admin-alias-inherits = erbt Einstellungen des Ziels

admin-reasoning-auto-resolved = Auto → { $style }

admin-badge-price = PREIS

admin-badge-budget = BUDGET

admin-badge-caps = FÄHIG

admin-badge-toml = TOML

admin-other-price-note = Sampling, Reasoning und Kontext gelten für diese Art nicht — nur Preise, für die Kostenabrechnung.

admin-toml-placeholder-header = # Häufige Schlüssel (vLLM/OpenAI):

admin-reasoning-style-label = Reasoning-Stil

admin-reasoning-style-aria = Reasoning-Stil

admin-reasoning-auto = Automatisch

admin-reasoning-none = keins

admin-reasoning-qwen = Qwen (vLLM)

admin-reasoning-openai = OpenAI

admin-reasoning-glm = GLM / z.AI

admin-reasoning-anthropic = Anthropic
admin-reasoning-ollama = Ollama

admin-effort-standard = Standard

admin-effort-deep = Tief

admin-effort-max = Max

admin-budget-placeholder = Standard

admin-budget-hint = Maximale Denk-Token pro Stufe. Leer = Backend-Standard (unbegrenzt). „Fast“ deaktiviert das Reasoning.

admin-effort-default-option = (Standard)

admin-effort-hint = Reasoning-Aufwand pro Stufe. Leer = eingebauter Standard. „Fast“ deaktiviert das Reasoning.

admin-saved-model = `{ $model }` gespeichert — sofort wirksam

admin-cleared-defaults = Overrides für `{ $model }` gelöscht

admin-price-label = { $cur }/{ $unit }

admin-price-unit-tokens = 1 Mio. Tokens

admin-price-unit-images = Bild

admin-price-unit-characters = Zeichen

admin-price-unit-seconds = Sekunde

admin-alias-chip = Alias

admin-defaults-intro = Das Modell, das genutzt wird, wenn eine Anfrage keines nennt — die Vorauswahl in den Chat-/Voice-Pickern und der API-Standard (anders als die Fallbacks für unbekannte Modelle auf der Upstreams-Seite, die greifen, wenn eine Anfrage ein Modell nennt, das kein Pool bereitstellt). Leer = das erste verfügbare Modell.

admin-defaults-chat-label = Chat

admin-defaults-voice-label = Sprache (Transkription)

admin-defaults-image-label = Bildgenerierung

admin-defaults-embedding-label = Embedding (RAG)

admin-defaults-first-option = Erstes verfügbares

admin-defaults-saved = Standardmodell auf `{ $model }` gesetzt

admin-defaults-cleared = Standardmodell zurückgesetzt

admin-capabilities-heading = Fähigkeiten

admin-cap-vision = Vision

admin-cap-structured-output = Strukturierte Ausgabe

admin-cap-audio-input = Audio-Eingabe

admin-cap-pdf-input = PDF-Eingabe

admin-cap-parallel-tools = Parallele Tools

admin-cap-unknown = Unbekannt

admin-cap-enabled = Aktiviert

admin-cap-disabled = Deaktiviert

admin-cap-fallback-vision = Fallback für Vision

admin-cap-fallback-tools = Fallback für Tools

admin-search-intro = Welches Backend das `search_web`-Tool des Assistenten beantwortet. SearXNG braucht nur eine Basis-URL und kostet pro Anfrage nichts, wenn du eine eigene Instanz betreibst; Brave braucht einen API-Key. Der Key wird verschlüsselt gespeichert.

admin-search-brave-key-set = Ein Key ist gespeichert (verschlüsselt).

admin-search-brave-key-unset = Kein Key gespeichert.

admin-search-brave-key-clear = Gespeicherten Key entfernen

# Herkunft des Kontextfensters. Der Wert wird vom Backend ermittelt, sofern es
# ihn meldet; überschreiben bleibt erlaubt, auch nach unten. Nur ein höherer
# Wert als der gemeldete ist eine Warnung wert: hier wird nicht kompaktiert,
# dort wird stillschweigend abgeschnitten.
admin-context-detected = Erkannt: { $window } Tokens
admin-context-unreported = Dieses Backend meldet für dieses Modell kein Kontextfenster. Hier eintragen oder am Server nachsehen.
admin-context-exceeds-detected = Dieses Backend meldet { $window } Tokens. Höhere Werte werden nicht kompaktiert, sondern vom Server still abgeschnitten.
auto-route-heading = Automatische Modellrouten
auto-route-description = Stellt einen standardkonformen Modellalias bereit, der für jede Anfrage das beste zulässige Modell auswählt.
auto-route-add = Automatische Route hinzufügen
auto-route-privacy = Der Selektor erhält den zur Klassifikation benötigten Anfrageinhalt, auch wenn das ausgewählte Generierungsmodell lokal läuft.
auto-route-empty = Es sind keine automatischen Routen konfiguriert.
auto-route-needs-selector = Fügen Sie einen System-One-Upstream hinzu, bevor Sie eine automatische Route erstellen.
auto-route-needs-candidates = Fügen Sie mindestens zwei unterschiedliche Chatmodelle hinzu, bevor Sie eine automatische Route erstellen.
auto-route-open-upstreams = Upstreams öffnen
auto-route-candidates = Kandidaten
auto-route-edit = Bearbeiten
auto-route-delete = Löschen
auto-route-delete-confirm = Automatische Route „{ $alias }“ löschen?
auto-route-deleted = Automatische Route „{ $alias }“ wurde gelöscht.
auto-route-saved = Automatische Route „{ $alias }“ wurde gespeichert.
auto-route-alias = Modellalias
auto-route-alias-help = Clients verwenden diesen Wert im standardmäßigen Modellfeld.
auto-route-selector = Selektormodell
auto-route-selector-help = Ein Modell aus einem System-One-Upstream, das einen Kandidatenschlüssel auswählt.
auto-route-editor-help = Beginnen Sie im Shadow-Modus, prüfen Sie die Routing-Entscheidungen und aktivieren Sie die Route, sobald ihre Richtlinie bereit ist.
auto-route-objective = Optimierungsziel
auto-route-objective-quality = Qualität
auto-route-objective-balanced = Ausgewogen
auto-route-objective-cost = Kosten
auto-route-rollout = Einführungsmodus
auto-route-rollout-shadow = Shadow (nur messen)
auto-route-rollout-active = Aktiv
auto-route-rollout-help = Shadow zeichnet Entscheidungen auf, leitet Anfragen aber zum Fallback. Aktiv wendet die Entscheidung des Selektors an.
auto-route-confidence = Mindestkonfidenz
auto-route-confidence-help = Entscheidungen mit geringerer Konfidenz verwenden das Fallback-Ziel.
auto-route-timeout = Selektor-Timeout (ms)
auto-route-instructions = Routing-Anweisungen
auto-route-candidate-add = Kandidat hinzufügen
auto-route-candidates-help = Beschreiben Sie jedes Modell anhand der passenden Anfragen. Der Selektor sieht Schlüssel und Beschreibung, nicht den Namen des Zielmodells.
auto-route-candidate-key = Undurchsichtiger Schlüssel
auto-route-candidate-target = Zielmodell oder statischer Alias
auto-route-candidate-description = Wann dieser Kandidat verwendet werden soll
auto-route-fallback = Fallback-Ziel
auto-route-session-affinity = Sitzung beim zuerst ausgewählten Modell halten
auto-route-session-affinity-help = Verwendet das effektive Ziel für denselben authentifizierten Client und dieselbe Sitzung bis zum Ablauf der TTL erneut.
auto-route-session-ttl = Sitzungsbindung TTL (Sekunden)
auto-route-cancel = Abbrechen
auto-route-save = Route speichern
auto-route-saving = Wird gespeichert…
auto-route-decisions = Letzte Routing-Entscheidungen
auto-route-result = Effektives Ziel
auto-route-latency = Selektor-Latenz
auto-route-reason-selected = Ausgewählt
auto-route-reason-shadow = Shadow-Vorschlag
auto-route-reason-low-confidence = Niedrige Konfidenz
auto-route-reason-selector-error = Selektorfehler
auto-route-reason-session-affinity = Sitzungsbindung
auto-route-error-unavailable = Kein konfiguriertes Ziel kann diese Anfrage derzeit verarbeiten. Prüfen Sie Verfügbarkeit und Fähigkeiten der Kandidaten und versuchen Sie es erneut.
auto-route-error-internal = Die automatische Route konnte nicht geladen werden. Prüfen Sie die Gateway-Protokolle und versuchen Sie es erneut.
auto-route-error-invalid-body = Die Anfrage für die automatische Route ist fehlerhaft. Prüfen Sie die übermittelten Felder und versuchen Sie es erneut.
auto-route-error-invalid-config = Die Konfiguration der automatischen Route ist ungültig. Prüfen Sie Kandidaten, Fallback, Konfidenz und Zeitlimits.
auto-route-error-nested = Automatische Routen können nicht auf andere automatische Routen verweisen. Wählen Sie statische Modell-Aliasse oder Modell-IDs.
auto-route-error-missing-alias = In der Anfrage-URL fehlt der Alias der automatischen Route.
auto-route-error-not-found = Die automatische Route existiert nicht. Aktualisieren Sie die Routenliste und versuchen Sie es erneut.
