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
admin-comfyui-reloaded = { $count } Workflow(s) neu geladen.
admin-comfyui-empty = Keine Workflows geladen — prüfe das Inhaltsverzeichnis.
admin-defaults-model-aria = Standardmodell für { $feature }
admin-defaults-set = Setzen
admin-search-provider-none = Keine
admin-add-overrides-heading = Modell-Überschreibungen hinzufügen
admin-edit-model-heading = { $model } bearbeiten
admin-add-model = Hinzufügen…
admin-pricing-unit-label = Preiseinheit
admin-pricing-unit-mtok = pro Mtok
admin-pricing-unit-ktok = pro Ktok
admin-pricing-unit-kimgs = pro 1000 Bilder
admin-clear-overrides-confirm = Alle gespeicherten Überschreibungen für { $model } verwerfen?
admin-users-col-email = E-Mail
