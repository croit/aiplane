# STATUS: llm-generated, unreviewed — pending native-speaker QA

rag-heading = RAG-Sammlungen

# Toasts — collection CRUD
rag-toast-vanished = Sammlung ist nach dem Speichern verschwunden.

# Toasts — refs / sources
rag-toast-bulk-queued-skipped = { $added } Quelle(n) eingeplant; { $skipped } Duplikat(e) übersprungen.
rag-toast-bulk-queued = Indexierung von { $added } Quelle(n) eingeplant.
rag-toast-reindex-queued-ref = Neuindexierung von `{ $ref }` eingeplant.

# Status badges
rag-status-pending = ausstehend
rag-status-cloning = klonen
rag-status-indexing = indexieren
rag-status-ready = bereit
rag-status-error = Fehler

# Collection row
rag-button-edit = Bearbeiten
rag-button-add-source = Quelle hinzufügen
rag-button-add-bulk = Quellen hinzufügen (Masse)

# Ref / source row
rag-badge-primary = primär
rag-button-reindex = Neu indexieren
rag-button-set-primary = Als primär festlegen
rag-button-remove = Entfernen

# Inline per-source editor
rag-label-branch-tag = Branch / Tag
rag-button-cancel = Abbrechen

# Create-collection form
rag-label-name = Name
rag-label-chunk-size = Chunk-Größe
rag-label-chunk-overlap = Chunk-Überlappung

# Edit-collection form
rag-label-description = Beschreibung

# Embedding model field
rag-label-embedding-model = Embedding-Modell

# Quellenauswahl + Zugangsdaten der Anbieter (rag_source.rs). Die
# Feldbeschriftungen der Anbieter stammen vom Anbieter selbst und werden
# nicht übersetzt.
rag-label-source-kind = Quelle
rag-source-unknown-kind = Unbekannte Quellenart.
rag-source-test-button = Verbindung testen
rag-source-test-ok = Verbunden als `{ $account }`. { $entries } Eintrag/Einträge im konfigurierten Ordner.
rag-source-test-ok-plain = Verbunden. { $entries } Eintrag/Einträge im konfigurierten Ordner.
rag-source-test-failed = Quelle nicht erreichbar: { $error }
rag-source-detected = Erkannt: { $server }

rag-label-profile = Dokumentfelder
rag-option-profile-none = Keine — nur Text indexieren

# Sync-Hook — ein eingehender Auslöser, der eine Sammlung neu synchronisiert.
rag-button-sync-token = Sync-URL
rag-badge-sync-hook = Sync-Hook

# Browser consent for an OAuth source (Google Drive).
rag-source-consent-save-first = Sammlung zuerst mit Client-ID und Secret speichern, dann verbinden, um Zugriff zu erteilen.
rag-oauth-lookup-failed = Die Sammlung konnte nicht gelesen werden.
rag-oauth-not-oauth = Diese Quellenart wird nicht im Browser verbunden.
rag-oauth-no-client = Speichern Sie zuerst OAuth-Client-ID und Secret an der Sammlung.
rag-oauth-bad-authorize-url = Die Autorisierungs-URL des Anbieters konnte nicht gebildet werden.
rag-oauth-start-failed = Die Autorisierung konnte nicht gestartet werden.
rag-oauth-callback-missing = In der Antwort des Anbieters fehlten Code oder State.
rag-oauth-expired = Diese Autorisierung ist abgelaufen oder wurde bereits verwendet. Bitte erneut starten.
rag-oauth-provider-refused = Der Anbieter hat die Autorisierung abgelehnt: { $error }
rag-oauth-exchange-failed = Der Tausch des Autorisierungscodes ist fehlgeschlagen: { $error }
rag-oauth-no-refresh-token = Der Anbieter hat kein Refresh-Token geliefert; unbeaufsichtigtes Indexieren wäre damit nicht möglich. Entziehen Sie dem Gateway im Anbieterkonto den Zugriff und verbinden Sie erneut.
rag-oauth-store-failed = Die Zugangsdaten konnten nicht gespeichert werden.

# Sammlungsverwaltung der SPA: Anlegeformular, Sync-URL-Karte, Quellzeilen und
# die Rückfragen, die die alte Seite nicht brauchte.
rag-button-new-collection = Neue Sammlung
rag-label-git-url = Git-URL
rag-source-testing = Wird getestet…
rag-button-create = Anlegen
rag-button-rebuild = Neu aufbauen
rag-sync-url-heading = Sync-URL — wird nur einmal angezeigt
rag-sync-token-confirm = Neue Sync-URL erzeugen? Die alte funktioniert dann nicht mehr.
rag-delete-collection-confirm = Sammlung { $name } samt Index löschen?
rag-remove-source-confirm = Quelle { $source } entfernen?
rag-toast-rebuild-queued = Vollständiger Neuaufbau angefordert.
rag-ref-indexed-at = indexiert { $date }
rag-no-sources = Keine Quellen — diese Sammlung indexiert nichts, bis eine hinzugefügt wird.
rag-add-sources-hint = Eine Quelle pro Zeile; ergänze { $at }, um den Ref { $ref } dieser Sammlung zu überschreiben.
