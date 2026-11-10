# STATUS: llm-generated, unreviewed — pending native-speaker QA


pools-field-name = Name
pools-field-kind = Art
pools-field-models = Bereitgestellte Modelle (kommagetrennt)
pools-field-backends = Backends
pools-save-pool = Pool speichern
pools-delete-pool = Löschen
pools-delete-confirm = Pool „{ $name }“ löschen? Seine Backends bleiben erhalten und werden nicht zugewiesen.

# Pool-Karten der SPA: die beiden Kurzübersichten und die Löschbestätigung.
pools-add-heading = Pool hinzufügen
pools-fallbacks-heading = Fallbacks für unbekannte Modelle
pools-fallbacks-description = Ersatz, wenn eine Anfrage ein Modell benennt, das kein Pool bereitstellt (anders als der Feature-Standard auf der Modelle-Seite, der greift, wenn eine Anfrage gar kein Modell nennt). Leer = der Fehltreffer gibt 404 zurück.
upstreams-problems-heading = Dieser Pool arbeitet nicht vollständig
pools-name-taken = Ein Pool mit diesem Namen existiert bereits — Speichern würde ihn ersetzen, samt Backends und Modellen. Wähle einen anderen Namen, um einen neuen Pool anzulegen.
pools-field-strategy = Strategie
pools-field-strategy-hint = prefix_affinity hält eine Konversation auf der Replika, die ihren KV-Cache schon hat (beste Wahl für Chat-/Agent-Traffic über mehrere GPUs — die anderen schicken aufeinanderfolgende Turns abwechselnd hin und her und zahlen jedes Mal den vollen Prefill) und verteilt trotzdem, wenn ein Backend echt stärker belastet ist. least_inflight verteilt nach aktueller Last; round_robin rotiert gewichtet.
pools-field-fallback-offline = Offline-Fallback-Modell
pools-field-fallback-offline-placeholder = wird ausgeliefert, wenn jedes Backend ausgefallen ist
pools-field-models-hint = Wenn gesetzt, werden genau diese IDs bereitgestellt und die Backends nicht gefragt, was sie haben — nötig, wo /models etwas anderes listet oder ein Modell auslässt. Leer = die Modelle über /models jedes Backends erkennen.
pools-field-models-chips = Von den Backends dieses Pools gemeldet — anklicken zum Festlegen oder Lösen
pools-field-allowed-groups = Erlaubte Gruppen
pools-field-allowed-groups-hint = AIplane-Gruppen, die die Modelle dieses Pools sehen + nutzen dürfen. Nichts ausgewählt = alle. Admins haben immer Zugriff. Gruppen unter Admin → Gruppen verwalten.
pools-field-voices = Stimmen (lang=voice pro Zeile)
pools-field-offer-voices = Auswählbare Stimmen (eine pro Zeile, Nutzer wählen)
pools-no-backends = Noch keine Backends definiert. Fügen Sie zuerst eines auf der Seite „Backends“ hinzu.
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
