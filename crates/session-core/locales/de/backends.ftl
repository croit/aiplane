# STATUS: llm-generated, unreviewed — pending native-speaker QA


backends-status-down = ausgefallen
backends-status-up = aktiv

backends-inflight-label = aktiv { $load }

# Backend-CRUD-Editor (Backends in der DB-Topologie hinzufügen/bearbeiten/löschen).
backends-apply-changes = Änderungen anwenden
backends-field-name = Name
backends-field-base-url = Basis-URL
backends-field-pool = Pool
backends-field-pool-none = (keiner)
backends-save-backend = Backend speichern
backends-delete-backend = Löschen

backends-field-api-key = API-Schlüssel
backends-field-api-key-keep = leer lassen, um den aktuellen Schlüssel zu behalten

# An alias that is configured but would not route.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.
backends-status-drained = Wartung

# Backend-Zeilen der SPA: Wartungsschalter, Last pro Stunde und Löschbestätigung.
backends-add-heading = Backend hinzufügen
backends-name-taken = Ein Backend mit diesem Namen existiert bereits — Speichern würde es überschreiben, samt Basis-URL, Schlüssel, Modellen und Pool. Wähle einen anderen Namen, um ein zweites Backend anzulegen.
backends-field-api-key-placeholder = API-Schlüssel (verschlüsselt gespeichert)
backends-field-api-key-env = API-Schlüssel-Umgebungsvariable
backends-field-health-path = Health-Pfad
backends-field-pool-hint = Weist dieses Backend einem Pool zu. Ein Backend in mehreren Pools wird auf den hier gewählten reduziert.
backends-field-weight = Gewichtung
backends-field-max-inflight = Max. gleichzeitig
backends-field-models = Modelle (kommagetrennt)
backends-field-aliases = Aliase (name=target pro Zeile)
backends-field-probe-models = Modelle über /models-Probe erkennen
backends-field-supports-edit = Unterstützt Bildbearbeitung
backends-status-saturated = ausgelastet
backends-auth-failed-title = Der Upstream hat die Zugangsdaten des Health-Probes abgelehnt (401/403), damit ist die Modell-Erkennung aus — über dieses Backend kann nichts Neues routbar werden. API-Schlüssel prüfen; bei einer Env-Var prüfen, ob die Variable überhaupt gesetzt ist.
backends-auth-failed = Key abgelehnt
backends-no-models-title = Dieses Backend meldet keine Modelle, also routet nichts dorthin und ein nackter Alias hat nichts, woran er binden könnte. Meist ein Health-Probe, der nie Daten geliefert hat.
backends-no-models = keine Modelle verfügbar
backends-key-env-badge = Key: env { $var }
backends-key-env-unset-badge = env { $var } NICHT GESETZT
backends-enabled-hint = Ausschalten, um dieses Backend für Wartungsarbeiten zu leeren. Wirkt sofort — kein „Änderungen anwenden“ nötig. Die Modelle bleiben bekannt, also übernehmen die anderen Backends des Pools und Clients sehen einen temporären Ausfall, nie „Modell nicht gefunden“.
backends-enabled-label = Nimmt Traffic an
backends-activity-summary = 15m { $m15 } · 30m { $m30 } · 60m { $m60 }
backends-aliases-label = Aliase:
backends-alias-target-title = Alias → { $target }
backends-alias-disabled-title = Bare-Alias deaktiviert — dieses Backend bedient mehrere Modelle; geben Sie ihm ein explizites Ziel (Zuordnungsformular)
backends-alias-disabled-label = { $name } (deaktiviert)
backends-fallback-offline-title = fallback_offline: wird ausgeliefert, wenn jedes Backend für ein bekanntes Modell in diesem Pool ausgefallen ist
backends-fallback-offline-badge = offline ↩ { $model }
backends-pool-empty = Keine Backends in diesem Pool.

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
backends-error-base-url-required = Basis-URL ist erforderlich

# Was die Erkennung über ein Backend ergeben hat.
backends-parallel-mismatch = Server verarbeitet { $parallel } gleichzeitig
backends-detect-profile = erkannt: { $profile }
