# STATUS: llm-generated, unreviewed — pending native-speaker QA


pools-field-name = Name
pools-field-kind = Art
pools-field-models = Bereitgestellte Modelle (Positivliste, kommagetrennt)
pools-field-backends = Backends
pools-save-pool = Pool speichern
pools-delete-pool = Löschen

# Pool-Karten der SPA: die beiden Kurzübersichten und die Löschbestätigung.
pools-add-heading = Pool hinzufügen
pools-fallbacks-heading = Fallbacks für unbekannte Modelle
pools-fallbacks-description = Ersatz, wenn eine Anfrage ein Modell benennt, das kein Pool bereitstellt (anders als der Feature-Standard auf der Modelle-Seite, der greift, wenn eine Anfrage gar kein Modell nennt). Leer = der Fehltreffer gibt 404 zurück.
upstreams-problems-heading = Dieser Pool arbeitet nicht vollständig
upstreams-coverage-heading = Was Clients sehen
upstreams-coverage-hint = Genau die Namen, die GET /v1/models für diesen Pool zurückgibt — je mit der Zahl der Backends, die ihn gerade bedienen können. Alles unter „voll“ heißt, dass für diesen Namen ein Teil deiner Hardware leerläuft.
upstreams-coverage-none-title = Kein Backend kann diesen Namen derzeit bedienen. Anfragen dafür erhalten einen temporären Ausfallfehler.
upstreams-coverage-partial-title = Nur ein Teil der Backends bedient diesen Namen — Anfragen dafür nutzen nur einen Teil des Pools, der Rest bleibt untätig. Meist ein Alias, dessen Ziel nicht zu dem passt, was ein Backend meldet.
upstreams-coverage-full-title = Jedes Backend dieses Pools bedient diesen Namen.
pools-name-taken = Ein Pool mit diesem Namen existiert bereits — Speichern würde ihn ersetzen, samt Backends und Modellen. Wähle einen anderen Namen, um einen neuen Pool anzulegen.
pools-field-strategy = Strategie
pools-field-strategy-hint = prefix_affinity hält eine Konversation auf der Replika, die ihren KV-Cache schon hat (beste Wahl für Chat-/Agent-Traffic über mehrere GPUs — die anderen schicken aufeinanderfolgende Turns abwechselnd hin und her und zahlen jedes Mal den vollen Prefill) und verteilt trotzdem, wenn ein Backend echt stärker belastet ist. least_inflight verteilt nach aktueller Last; round_robin rotiert gewichtet.
pools-field-fallback-offline = Offline-Fallback-Modell
pools-field-fallback-offline-placeholder = wird ausgeliefert, wenn jedes Backend ausgefallen ist
pools-field-models-hint = Wenn gesetzt, werden von einem Backend mit /models-Probe nur diese IDs bereitgestellt — der Rest wird durchgestrichen angezeigt. Leer = alles bereitstellen, was das Backend meldet.
pools-field-allowed-groups = Erlaubte Gruppen
pools-field-allowed-groups-hint = AIplane-Gruppen, die die Modelle dieses Pools sehen + nutzen dürfen. Nichts ausgewählt = alle. Admins haben immer Zugriff. Gruppen unter Admin → Gruppen verwalten.
pools-field-voices = Stimmen (lang=voice pro Zeile)
pools-field-offer-voices = Auswählbare Stimmen (eine pro Zeile, Nutzer wählen)
pools-no-backends = Noch keine Backends definiert. Fügen Sie zuerst eines auf der Seite „Backends“ hinzu.
pools-field-gdpr = GDPR-konform
pools-field-nda = NDA-abgedeckt
pools-field-enforce-limits = Ratenlimits & Kontingente durchsetzen

upstreams-problem-no-backends = Keine Backends zugeordnet — in diesem Pool kann nichts eine Anfrage bedienen.
upstreams-problem-all-drained = Alle Backends sind für Wartung geleert, es routet nichts hierher.
upstreams-problem-all-down = Kein Backend verfügbar: Anfragen warten auf eine Rückkehr und erhalten dann einen temporären Ausfallfehler.
upstreams-problem-auth = Zugangsdaten abgelehnt von: { $backends }. Dort ist die Modell-Erkennung aus, diese Backends melden nichts.
upstreams-problem-no-models = Keine Modelle gemeldet von: { $backends }. Dorthin routet nichts, und ein nackter Alias findet dort nichts, woran er binden kann.
upstreams-problem-broken-aliases = Aliase, die nirgendwohin routen: { $aliases }. Jeder zeigt auf ein Modell, das sein Backend nicht bedient, oder hat nichts zum Binden.
upstreams-problem-partial-coverage = Nur von einem Teil des Pools bedient: { $models }. Anfragen auf diese Namen nutzen weniger Replikas als vorhanden.
upstreams-problem-unserved-allowlist = Unter „Modelle“ eingetragen, aber von niemandem bedient: { $models }.
upstreams-problem-missing-backends = Zugeordnete Backends, die es nicht mehr gibt: { $backends }.
upstreams-apply-diff-summary = Zeigen, was das Anwenden ändert
upstreams-diff-pool-added = neuer Pool { $pool } nimmt den Betrieb auf
upstreams-diff-pool-removed = Pool { $pool } stellt den Betrieb ein
upstreams-diff-pool-kind = Pool { $pool }: Art { $from } → { $to }
upstreams-diff-pool-strategy = Pool { $pool }: Strategie { $from } → { $to }
upstreams-diff-backend-joins = { $backend } kommt in Pool { $pool } und nimmt Traffic an
upstreams-diff-backend-leaves = { $backend } verlässt Pool { $pool } und nimmt keinen Traffic mehr an
upstreams-diff-backend-url = { $backend }: Basis-URL { $from } → { $to } (die erkannten Modelle werden neu geprobt)
upstreams-diff-backend-limits = { $backend }: Gewicht { $weight }, max. parallel { $inflight }
upstreams-diff-backend-health-path = { $backend }: Health-Pfad → { $to }
