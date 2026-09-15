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

admin-page-title = Modelle — LLM Gateway

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
admin-context-unreported = Dieses Backend meldet sein Kontextfenster nicht. Am Server prüfen — `ollama ps` zeigt die tatsächlich allozierte Größe.
admin-context-exceeds-detected = Dieses Backend meldet { $window } Tokens. Höhere Werte werden nicht kompaktiert, sondern vom Server still abgeschnitten.
