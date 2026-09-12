# STATUS: llm-generated, unreviewed — pending native-speaker QA

rag-heading = RAG-Sammlungen
rag-description-prefix = Codebasen, die das Gateway indexiert hat. Das Werkzeug
rag-description-suffix = greift auf diese Sammlungen zu, um Fragen zum Code zu beantworten.
rag-collections-heading = Konfigurierte Sammlungen
rag-empty-list = Noch keine Sammlungen. Erstellen Sie oben eine.

# Toasts — collection CRUD
rag-toast-indexing-queued = Indexierung von `{ $name }` @ `{ $ref }` wurde eingeplant.
rag-toast-created-aggregate = `{ $name }` (Aggregat) erstellt. Fügen Sie unten Quell-Repos hinzu, um sie zu indexieren.
rag-toast-collection-saved = `{ $name }` gespeichert.
rag-toast-vanished = Sammlung ist nach dem Speichern verschwunden.
# Toasts — refs / sources
rag-toast-bulk-queued-skipped = { $added } Quelle(n) eingeplant; { $skipped } Duplikat(e) übersprungen.
rag-toast-bulk-queued = Indexierung von { $added } Quelle(n) eingeplant.
rag-toast-source-updated = Quelle aktualisiert.

# Status badges
rag-status-pending = ausstehend
rag-status-cloning = klonen
rag-status-indexing = indexieren
rag-status-ready = bereit
rag-status-error = Fehler

# Collection row
rag-pat-set = PAT gesetzt
rag-pat-none = kein PAT
rag-meta-aggregate = { $count } Quelle(n) · { $hint }
rag-meta-versioned = { $url } · { $hint }
rag-badge-aggregate = Aggregat
rag-embed-prefix = Embed:
rag-button-edit = Bearbeiten
rag-button-delete-collection = Sammlung löschen
rag-placeholder-source-git-url = https://github.com/org/repo.git
rag-button-add-source = Quelle hinzufügen
rag-placeholder-branch-tag-commit = Branch, Tag oder Commit
rag-button-add-ref = Ref hinzufügen
rag-placeholder-bulk-sources = Massenhinzufügung — ein Repo pro Zeile, optional @ref:
    https://github.com/proxmox/pve-manager.git
    https://github.com/proxmox/qemu-server.git @master
rag-button-add-bulk = Quellen hinzufügen (Masse)

# Ref / source row
rag-badge-primary = primär
rag-ref-indexed-line = indexiert { $date } · { $commit }
rag-never = nie
rag-button-log = Protokoll
rag-button-reindex = Neu indexieren
rag-button-set-primary = Als primär festlegen
rag-button-remove = Entfernen

# Indexing log
rag-log-info = Info
rag-log-warn = Warnung
rag-log-error = Fehler
rag-log-heading = Indexierungsprotokoll
rag-log-empty = Noch keine Indexierungsereignisse aufgezeichnet. Der erste Lauf protokolliert hier, sobald der Indexer diesen Ref aufgreift.

# Inline per-source editor
rag-label-git-url-source = Git-URL (diese Quelle)
rag-label-git-url-inherit = Git-URL (leer = von Sammlung übernehmen)
rag-placeholder-git-url = https://example.com/org/repo.git
rag-label-branch-tag = Branch / Tag
rag-button-save-source = Quelle speichern
rag-button-cancel = Abbrechen

# Create-collection form
rag-create-heading = Neue Sammlung indexieren
rag-create-description = Der Indexer klont das Repo, zerlegt jede Datei in Chunks und embeddet sie mit dem konfigurierten Embedding-Modell. PATs werden im Klartext gespeichert (das Gateway läuft auf vertrauenswürdiger Infrastruktur).
rag-new-page-title = Neue Sammlung indexieren
rag-edit-page-title = Sammlung bearbeiten
rag-not-found = Keine Sammlung mit dieser ID. Sie wurde möglicherweise gelöscht.
rag-back-to-collections = RAG-Sammlungen
rag-edit-source-heading = Quelle bearbeiten
rag-add-source-heading = Quelle hinzufügen
rag-label-name = Name
rag-placeholder-name = z. B. gateway-repo
rag-label-description-optional = Beschreibung (optional)
rag-placeholder-description = kurz, gut lesbar
rag-label-git-url-versioned = Git-URL (nur versioniert)
rag-label-pat-optional = Persönlicher Zugriffstoken (optional)
rag-placeholder-pat = für private Repos
rag-label-include-globs-full = Include-Globs (kommagetrennt oder zeilenweise)
rag-placeholder-include-globs = *.rs, *.md
rag-label-exclude-globs = Exclude-Globs
rag-placeholder-exclude-globs = target/, node_modules/
rag-label-chunk-size = Chunk-Größe
rag-label-chunk-overlap = Chunk-Überlappung
rag-label-allowed-groups = Erlaubte Gruppen
rag-hint-allowed-groups = Kommagetrennte Gateway-Gruppen, die diese Collection auflisten + durchsuchen dürfen. Leer = alle mit den RAG-Tools. Admins haben immer Zugriff.
rag-create-aggregate-help = Aggregat (Multi-Quelle): durchsucht viele Repos als einen Korpus. Lassen Sie die Git-URL leer und fügen Sie nach dem Erstellen jedes Quell-Repo hinzu. Branch / Tag wird zum Standard-Ref für hinzugefügte Quellen.
rag-button-queue-indexing = Indexierung einplanen

# Edit-collection form
rag-edit-heading = Bearbeite { $name }
rag-label-description = Beschreibung
rag-label-pat = Persönlicher Zugriffstoken
rag-placeholder-pat-keep = leer lassen, um bestehenden zu behalten
rag-label-clear-pat = Gespeicherten PAT entfernen (nicht mehr authentifizieren)
rag-label-include-globs = Include-Globs
rag-button-save-changes = Änderungen speichern

# Embedding model field
rag-label-embedding-model = Embedding-Modell
rag-placeholder-embedding-model-none = keine Embedding-Pools konfiguriert — Modell-ID eingeben
rag-option-choose-embedding-model = Embedding-Modell wählen…
rag-suffix-not-advertised = (nicht mehr verfügbar)

# Quellenauswahl + Zugangsdaten der Anbieter (rag_source.rs). Die
# Feldbeschriftungen der Anbieter stammen vom Anbieter selbst und werden
# nicht übersetzt.
rag-label-source-kind = Quelle
rag-source-git-help = Klont ein Repository und indexiert dessen Dateien. Das bisherige Verhalten.
rag-source-secret-placeholder = leer lassen, um den gespeicherten Wert zu behalten
rag-source-unknown-kind = Unbekannte Quellenart.
rag-source-test-button = Verbindung testen
rag-source-test-ok = Verbunden als `{ $account }`. { $entries } Eintrag/Einträge im konfigurierten Ordner.
rag-source-test-ok-plain = Verbunden. { $entries } Eintrag/Einträge im konfigurierten Ordner.
rag-source-test-failed = Quelle nicht erreichbar: { $error }
rag-source-test-git = Wähle eine entfernte Quelle zum Testen. Git-Repositories werden beim Indexieren geprüft.
rag-source-detected = Erkannt: { $server }

rag-label-profile = Dokumentfelder
rag-option-profile-none = Keine — nur Text indexieren
rag-profile-help = Extrahiert Felder (Lieferant, Datum, Betrag, Projekt) aus jedem Dokument, damit sie gefiltert, sortiert und summiert werden können. Kostet einen Modellaufruf pro Dokument; für Code- oder reine Textsammlungen "Keine" lassen.

# Editor für Extraktionsprofile (/rag/profiles, rag_profiles.rs)
rag-profile-heading = Extraktionsprofile
rag-profile-description = Was aus jedem Dokument einer Sammlung extrahiert wird: die Felder, mit denen "die letzte Rechnung von X" oder "wie viel haben wir ausgegeben" überhaupt beantwortbar werden. Ein Profil wird einer Sammlung auf der RAG-Seite zugewiesen.
rag-profile-create-heading = Neues Profil
rag-profile-list-heading = Profile
rag-profile-empty = Noch keine Profile.
rag-profile-builtin = mitgeliefert
rag-profile-version = v{ $version }
rag-profile-summary = { $count } Feld(er)
rag-profile-label-name = Name
rag-profile-label-description = Beschreibung
rag-profile-label-prompt = Extraktionsanweisungen
rag-profile-label-fields = Felder (JSON)
rag-profile-prompt-placeholder = Beschreibe, was das Modell liest und wie Datums- und Betragsangaben zu normalisieren sind.
rag-profile-fields-help = Ein Objekt pro Feld: key, label, type (text | number | date | enum), description sowie optional filterable / sortable. Ein enum braucht zusätzlich "values". Die Beschreibung sieht das Modell — also präzise formulieren.
rag-profile-edit-warning = Beim Speichern wird die Profilversion erhöht und der Extraktions-Cache verworfen. Sammlungen, die dieses Profil nutzen, müssen neu indexiert werden.
rag-profile-button-create = Profil anlegen
rag-profile-button-save = Speichern
rag-profile-button-delete = Löschen
rag-profile-delete-confirm = Profil { $name } löschen?
rag-profile-example-counterparty-label = Gegenpartei
rag-profile-example-counterparty-description = Die andere Vertragspartei.
rag-profile-example-date-label = Datum
rag-profile-example-date-description = Das Dokumentdatum.
rag-profile-example-amount-label = Betrag
rag-profile-example-amount-description = Der Gesamtbetrag.
rag-profile-link = Extraktionsprofile bearbeiten
rag-profile-toast-created = Profil `{ $name }` angelegt.
rag-profile-toast-saved = `{ $name }` gespeichert.
rag-profile-toast-saved-reindex = `{ $name }` gespeichert. Zum Anwenden neu indexieren: { $collections }.
rag-profile-toast-deleted = Profil gelöscht.
# Sync-Hook — ein eingehender Auslöser, der eine Sammlung neu synchronisiert.
rag-toast-sync-token = Sync-URL (wird einmalig angezeigt und nicht gespeichert): { $url }
rag-toast-sync-token-cleared = Sync-URL deaktiviert.
rag-button-sync-token = Sync-URL
rag-button-sync-token-rotate = Neue Sync-URL
rag-button-sync-token-clear = Sync-URL deaktivieren
rag-badge-sync-hook = Sync-Hook

# Browser consent for an OAuth source (Google Drive).
rag-source-consent-save-first = Sammlung zuerst mit Client-ID und Secret speichern, dann verbinden, um Zugriff zu erteilen.
rag-source-consent-connected = verbunden
rag-source-consent-connect = Verbinden
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
rag-badge-no-files = keine Dateien indexiert
rag-ref-files = { $files } Dateien
rag-label-git-url = Git-URL
rag-source-testing = Wird getestet…
rag-sync-url-heading = Sync-URL — wird nur einmal angezeigt
rag-sync-token-confirm = Neue Sync-URL erzeugen? Die alte funktioniert dann nicht mehr.
rag-delete-collection-confirm = Sammlung { $name } samt Index löschen?
rag-remove-source-confirm = Quelle { $source } entfernen?
rag-toast-rebuild-queued = Vollständiger Neuaufbau angefordert.
rag-no-sources = Keine Quellen — diese Sammlung indexiert nichts, bis eine hinzugefügt wird.
rag-add-sources-hint = Eine Quelle pro Zeile; ergänze { $at }, um den Ref { $ref } dieser Sammlung zu überschreiben.
