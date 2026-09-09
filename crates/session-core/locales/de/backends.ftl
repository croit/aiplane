# STATUS: llm-generated, unreviewed — pending native-speaker QA

backends-heading = Upstream-Backends

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
backends-add-backend = Backend hinzufügen
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
backends-drain-button = Entleeren
backends-undrain-button = Wieder aufnehmen
backends-requests-per-hour = { $count } Anfr./h
backends-delete-confirm = Backend { $name } löschen? Danach die Topologie übernehmen.
