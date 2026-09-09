# STATUS: llm-generated, unreviewed — pending native-speaker QA

backends-page-title = Upstream-Backends — LLM Gateway
backends-heading = Upstream-Backends
backends-description-prefix = Live-Ansicht der konfigurierten Upstream-Pools — Zustand, aktuelle Auslastung im Verhältnis zur Obergrenze jedes Backends und die Modelle, die jedes davon aktuell anbietet. Nur lesend: Das Routing richtet sich ausschließlich danach, was die Backends über ihre
backends-description-suffix = Probe melden.
backends-summary = { $total } Backends · { $healthy } gesund · { $down } ausgefallen
backends-unknown-fallback-prefix = Fallback für unbekanntes Modell —
backends-empty-prefix = Keine Upstream-Pools konfiguriert. Fügen Sie einen
backends-empty-suffix = Block zur gateway.toml hinzu und starten Sie neu.

backends-fallback-offline-title = fallback_offline: wird ausgeliefert, wenn jedes Backend für ein bekanntes Modell in diesem Pool ausgefallen ist
backends-fallback-offline-badge = offline ↩ { $model }
backends-pool-empty = Keine Backends in diesem Pool.

backends-status-down = ausgefallen
backends-status-saturated = ausgelastet
backends-status-up = aktiv

backends-inflight-label = aktiv { $load }
backends-activity-summary = 15m { $m15 } · 30m { $m30 } · 60m { $m60 }
backends-no-models = keine Modelle verfügbar
backends-aliases-label = Aliase:

backends-alias-target-title = Alias → { $target }
backends-alias-disabled-label = { $name } (deaktiviert)
backends-alias-disabled-title = Bare-Alias deaktiviert — dieses Backend bedient mehrere Modelle; geben Sie ihm ein explizites Ziel (Zuordnungsformular)
backends-alias-bare-title = Alias → Modell dieses Backends

# Backend-CRUD-Editor (Backends in der DB-Topologie hinzufügen/bearbeiten/löschen).
backends-manage-heading = Backends verwalten
backends-manage-description = Upstream-Backends hinzufügen, bearbeiten oder entfernen. Änderungen werden in der Datenbank gespeichert, werden aber erst wirksam, wenn Sie auf „Änderungen anwenden“ klicken.
backends-apply-changes = Änderungen anwenden
backends-add-heading = Backend hinzufügen
backends-field-name = Name
backends-field-base-url = Basis-URL
backends-field-api-key-env = API-Schlüssel-Umgebungsvariable
backends-field-health-path = Health-Pfad
backends-field-weight = Gewichtung
backends-field-max-inflight = Max. gleichzeitig
backends-field-pool = Pool
backends-field-pool-none = (keiner)
backends-field-pool-hint = Weist dieses Backend einem Pool zu. Ein Backend in mehreren Pools wird auf den hier gewählten reduziert.
backends-field-models = Modelle (kommagetrennt)
backends-field-aliases = Aliase (name=target pro Zeile)
backends-field-probe-models = Modelle über /models-Probe erkennen
backends-field-supports-edit = Unterstützt Bildbearbeitung
backends-save-backend = Backend speichern
backends-add-backend = Backend hinzufügen
backends-delete-backend = Löschen
backends-error-name-required = Backend-Name ist erforderlich
backends-error-base-url-required = Basis-URL ist erforderlich
backends-saved = Backend `{ $name }` gespeichert — klicken Sie auf „Änderungen anwenden“, um neu zu laden
backends-deleted = Backend `{ $name }` gelöscht — klicken Sie auf „Änderungen anwenden“, um neu zu laden

backends-field-api-key = API-Schlüssel
backends-field-api-key-placeholder = API-Schlüssel (verschlüsselt gespeichert)
backends-field-api-key-keep = leer lassen, um den aktuellen Schlüssel zu behalten

# Duplicate-name guard on the Add-backend form.
backends-error-name-exists = ein Backend namens `{ $name }` existiert bereits — klicken Sie erneut auf „Backend hinzufügen“, um es zu überschreiben, oder ändern Sie den Namen
backends-overwrite-hint = Dieser Name existiert bereits. Erneutes Speichern ÜBERSCHREIBT das vorhandene Backend — Basis-URL, API-Schlüssel, Modelle, Aliase und Pool-Zuordnung. Ändern Sie den Namen, um stattdessen ein zweites Backend anzulegen.

# An alias that is configured but would not route.
backends-alias-unresolved-title = DEFEKT: Dieser Alias zeigt auf `{ $target }`, was dieses Backend nicht bedient — Anfragen darauf schlagen fehl.
backends-alias-nothing-title = DEFEKT: Dieses Backend meldet keine Modelle, ein nackter Alias hat nichts, woran er binden könnte.
backends-alias-serves = Bedient wird: { $models }
backends-alias-serves-nothing = Derzeit werden keine Modelle bedient.
# Save-time check on the aliases textarea.
backends-alias-target-unknown = gespeichert, aber diese Alias-Ziele werden von diesem Backend nicht bedient: { $targets } — bedient wird { $models }. Diese Aliase routen nicht, bis das Ziel exakt übereinstimmt.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
backends-auth-failed = Key abgelehnt
backends-auth-failed-title = Der Upstream hat die Zugangsdaten des Health-Probes abgelehnt (401/403), damit ist die Modell-Erkennung aus — über dieses Backend kann nichts Neues routbar werden. API-Schlüssel prüfen; bei einer Env-Var prüfen, ob die Variable überhaupt gesetzt ist.
backends-no-models-title = Dieses Backend meldet keine Modelle, also routet nichts dorthin und ein nackter Alias hat nichts, woran er binden könnte. Meist ein Health-Probe, der nie Daten geliefert hat.
# Maintenance switch.
backends-enabled-label = Nimmt Traffic an
backends-enabled-hint = Ausschalten, um dieses Backend für Wartungsarbeiten zu leeren. Wirkt sofort — kein „Änderungen anwenden“ nötig. Die Modelle bleiben bekannt, also übernehmen die anderen Backends des Pools und Clients sehen einen temporären Ausfall, nie „Modell nicht gefunden“.
backends-enabled-on = Backend `{ $name }` nimmt wieder Traffic an
backends-enabled-off = Backend `{ $name }` für Wartung geleert — es werden keine neuen Anfragen mehr dorthin geroutet
backends-status-drained = Wartung
backends-status-drained-title = Für Wartung geleert: das Backend ist erreichbar, der Router überspringt es aber. „Nimmt Traffic an“ wieder einschalten, um es zurück in die Rotation zu nehmen.
# "Test connection": call the upstream with what is typed in the editor.
backends-test-button = Verbindung testen
backends-test-hint = Ruft diese URL mit den obigen Zugangsdaten auf. Es wird nichts gespeichert.
backends-test-insert-hint = Gemeldete Modell-Ids — klicken, um die Alias-Zeile am Cursor zu vervollständigen:
backends-test-ok = Erreichbar, authentifiziert ({ $source }), { $count } Modelle gefunden.
backends-test-ok-no-models = Erreichbar und authentifiziert ({ $source }), aber die Antwort ist kein OpenAI-/models-Envelope — die Erkennung kann sie nicht lesen. Dieses Backend kann nur die Ids bedienen, die du unter „Modelle“ einträgst.
backends-test-auth-failed = Abgelehnt mit HTTP { $status }: die Zugangsdaten wurden verweigert ({ $source }). Bis das behoben ist, bleibt die Modell-Erkennung aus und das Backend meldet nichts.
backends-test-http-error = { $url } antwortete mit HTTP { $status }.
backends-test-unreachable = { $url } nicht erreichbar: { $err }
backends-test-timeout = { $url } hat nicht innerhalb von { $secs }s geantwortet.
backends-test-key-typed = mit dem oben eingetippten Schlüssel
backends-test-key-stored = mit dem gespeicherten Schlüssel
backends-test-key-env = aus env { $var }
backends-test-key-env-unset = env { $var } ist NICHT GESETZT — die Anfrage ging ohne Zugangsdaten raus
backends-test-key-none = keine Zugangsdaten gesendet
# Where the API key comes from (U5).
backends-key-env-badge = Key: env { $var }
backends-key-env-title = Dieses Backend hat keinen gespeicherten Schlüssel und liest ihn aus dieser Umgebungsvariablen, die derzeit gesetzt ist.
backends-key-env-unset-badge = env { $var } NICHT GESETZT
backends-key-env-unset-title = Dieses Backend hat keinen gespeicherten Schlüssel, und die genannte Umgebungsvariable ist im Gateway-Prozess nicht gesetzt — es werden gar keine Zugangsdaten gesendet. Wenn der Upstream welche verlangt, bekommt jeder Probe 401, die Modell-Erkennung bleibt aus und das Backend meldet nichts. Trage den Schlüssel stattdessen im Feld „API-Schlüssel“ ein, oder setze die Variable und starte neu.
# Live name-clash note on the add form (U9).
backends-name-taken = Ein Backend mit diesem Namen existiert bereits — Speichern würde es überschreiben, samt Basis-URL, Schlüssel, Modellen und Pool. Wähle einen anderen Namen, um ein zweites Backend anzulegen.

# Backend-Zeilen der SPA: Wartungsschalter, Last pro Stunde und Löschbestätigung.
backends-drain-button = Entleeren
backends-undrain-button = Wieder aufnehmen
backends-requests-per-hour = { $count } Anfr./h
backends-delete-confirm = Backend { $name } löschen? Danach die Topologie übernehmen.
