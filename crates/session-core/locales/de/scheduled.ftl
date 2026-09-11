# STATUS: llm-generated, unreviewed — pending native-speaker QA


scheduled-heading = Geplante Aktionen
scheduled-intro = Lass einen Prompt automatisch nach einem Zeitplan ausführen. Jeder Lauf öffnet einen neuen Chat, den du hier lesen kannst — wähle ein Modell, schreibe den Prompt und lege fest, wann er ausgeführt werden soll.
scheduled-create-submit = Geplante Aktion erstellen
scheduled-list-heading = Deine geplanten Aktionen
scheduled-list-empty = Noch keine geplanten Aktionen. Erstelle oben eine.

scheduled-back = Zurück
scheduled-edit-heading = Geplante Aktion bearbeiten
scheduled-save-submit = Änderungen speichern

scheduled-name-label = Name
scheduled-name-placeholder = z. B. Tägliche News-Zusammenfassung
scheduled-model-label = Modell
scheduled-model-placeholder = Modell-ID (z. B. gpt-4o-mini)
scheduled-gdpr-warning = Dieses Modell ist nicht DSGVO-konform. Geplante Läufe senden deinen Prompt automatisch an dieses Modell — vermeide personenbezogene Daten.
scheduled-nda-warning = Dieses Modell ist nicht durch eine Vertraulichkeitsvereinbarung abgedeckt. Plane keine NDA-geschützten oder proprietären Inhalte für dieses Modell.
scheduled-prompt-label = Prompt
scheduled-prompt-placeholder = Was soll das Modell bei jedem Lauf tun?
scheduled-tools-toggle-label = Werkzeuge erlauben (Websuche, RAG, Anhänge) — wie im Chat
scheduled-reuse-toggle-label = Den Chat des vorherigen Laufs wiederverwenden — jeder Lauf setzt dieselbe Unterhaltung fort
scheduled-reuse-rounds-prefix = die letzten
scheduled-reuse-rounds-aria = Anzahl der zu wiederholenden Verlaufsrunden
scheduled-reuse-rounds-suffix = Runden senden

scheduled-builder-heading = Zeitplan
scheduled-mode-hourly = Stündlich
scheduled-mode-daily = Täglich
scheduled-mode-weekly = Wöchentlich
scheduled-mode-monthly = Monatlich
scheduled-mode-advanced = Erweitert
scheduled-weekday-mon = Mo
scheduled-weekday-tue = Di
scheduled-weekday-wed = Mi
scheduled-weekday-thu = Do
scheduled-weekday-fri = Fr
scheduled-weekday-sat = Sa
scheduled-weekday-sun = So
scheduled-on-day-label = Am Tag
scheduled-of-every-month = jeden Monats
scheduled-at-label = Um
scheduled-hour-aria = Stunde
scheduled-minute-aria = Minute
scheduled-of-every-hour = jeder Stunde
scheduled-timezone-label = Zeitzone
scheduled-timezone-placeholder = Europe/Berlin
scheduled-cron-label = Cron-Ausdruck
scheduled-cron-help = Fünf Felder: Minute Stunde Tag-des-Monats Monat Wochentag.

scheduled-no-upcoming-runs = Keine bevorstehenden Läufe.
scheduled-next-runs-prefix = Nächste Läufe:{ " " }

scheduled-err-pick-weekday = Wähle mindestens einen Wochentag.
scheduled-err-enter-cron = Gib einen Cron-Ausdruck ein.

scheduled-model-non-gdpr = { $model } (nicht DSGVO-konform)
scheduled-model-nda-restricted = { $model } (vertraulichkeitsbeschränkt)
scheduled-model-non-gdpr-nda-restricted = { $model } (nicht DSGVO-konform, vertraulichkeitsbeschränkt)

scheduled-toast-not-found = Keine solche geplante Aktion.

scheduled-badge-active = aktiv
scheduled-badge-paused = pausiert
scheduled-status-paused = Pausiert
scheduled-next-run = Nächster Lauf: { $when }
scheduled-no-upcoming-run = Kein bevorstehender Lauf
scheduled-last-success = Letzter: ✓ { $when }
scheduled-last-success-open = Letzter: ✓ { $when } — öffnen
scheduled-last-failure = Letzter: ✗ { $when }
scheduled-last-failure-open = Letzter: ✗ { $when } — öffnen
scheduled-pause-title = Pausieren
scheduled-resume-title = Fortsetzen
scheduled-edit-title = Bearbeiten
scheduled-delete-title = Löschen
scheduled-delete-confirm = Diese geplante Aktion löschen?
scheduled-preview-summary = { $summary } ({ $timezone })
scheduled-preview-next-runs = Nächste Ausführungen: { $runs }
scheduled-last-error = Letzter Fehler: { $error }
