# Admin-Editor für Ratenlimits / Kontingente (/admin/limits).
limits-heading = Ratenlimits & Kontingente
limits-add-heading = Limit hinzufügen oder aktualisieren
limits-field-subject = Gilt für
limits-field-model = Modell
limits-field-dimension = Limit
limits-field-window = Pro
limits-field-value = Wert
limits-add-submit = Limit speichern
limits-subject-global = Alle (Vorgabe)
limits-subject-role = Rolle
limits-subject-user = Benutzer
limits-dim-requests = Anfragen
limits-dim-tokens = Token
limits-dim-cost = Kosten ({ $cur })
limits-dim-cost-short = Kosten
limits-win-hour = Stunde
limits-win-day = Tag
limits-win-week = Woche
limits-win-month = Monat
limits-col-subject = Gilt für
limits-col-scope = Modell
limits-col-limit = Limit
limits-col-window = Zeitfenster
limits-none = Keine Limits konfiguriert — jeder ist unbegrenzt.
limits-all-models = alle Modelle
limits-delete = Löschen
limits-edit = Bearbeiten
limits-edit-heading = Limit bearbeiten
limits-edit-submit = Änderungen speichern
limits-saved = Limit für { $subject } gespeichert
limits-subject-token = API-Token
limits-subject-system = Agent

# Regeltabelle der SPA: Überschrift, Spalte „verwaltet von“ und Löschabfrage.
limits-delete-confirm = Diese Regel löschen?
limits-intro = Begrenzen Sie, wie viele Anfragen, wie viele Token oder wie viel Ausgaben ein Aufrufer über ein gleitendes Zeitfenster nutzen darf. Regeln werden von der spezifischsten zur allgemeinsten aufgelöst: Die eigene Regel eines Benutzers gewinnt, sonst die großzügigste seiner Rollen, sonst die globale Vorgabe. Ohne Regeln ist jeder unbegrenzt. Eine Regel für ein API-Token ist eine zusätzliche Obergrenze, die neben dem Budget des Besitzers geprüft wird — sie kann den Verbrauch dieses Tokens also nur enger fassen. Nur abgerechnete Pools zählen (selbst gehostete Pools mit enforce_limits = false sind ausgenommen), und das gesamte Budget eines Benutzers wird über seine API-Tokens, den Chat und geplante Ausführungen hinweg geteilt. Eine auf ein Modell (oder eine `prefix-*`-Familie) eingegrenzte Regel ersetzt das Aggregat in ihrem Zeitfenster: Für einen Aufruf dieses Modells zählt nur die spezifischste passende Regel — außer das Aggregat ist als Gesamtlimit markiert, dann deckelt es zusätzlich.
limits-field-subject-id = Rolle / Benutzer / Token / Agent
limits-field-subject-id-ph = Auswählen …
limits-col-value = Wert
limits-col-actions = Aktionen
limits-deleted = Limit entfernt
limits-field-total = Gesamtlimit
limits-field-total-hint = Wenn gesetzt, deckelt diese Regel zusätzlich — auch dort, wo eine spezifischere Regel mehr erlaubt. Wenn nicht gesetzt, gewinnt für das aufgerufene Modell die spezifischste passende Regel.
limits-field-model-hint = Leer bedeutet alle Modelle; ein * am Ende fasst eine Familie (z. B. claude-*).
limits-col-kind = Art
limits-kind-total = Gesamt
limits-kind-override = Ausnahme
