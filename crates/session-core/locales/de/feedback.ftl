# Das Feedback-Widget: der schwebende Button, der Dialog (Formular,
# Spracheingabe, Screenshot-Annotation, Anhänge, Diagnose-Einwilligung), die
# Bestätigung zum öffentlichen Tracker und die `/api/v0/feedback*`-Fehler.

feedback-fab-aria = Feedback senden
feedback-fab-title = Feedback senden

feedback-dialog-heading = Feedback senden
feedback-dialog-blurb = Die Seite, dein Browser sowie die jüngsten Konsolen- und Netzwerkaktivitäten werden automatisch angehängt.
feedback-close-aria = Schließen
feedback-cancel-button = Abbrechen
feedback-submit-button = Feedback senden
feedback-sending = Wird gesendet …

feedback-title-label = Titel
feedback-title-placeholder = Kurze Zusammenfassung
feedback-description-label = Beschreibung
feedback-description-placeholder = Was ist passiert oder was wünschst du dir?
feedback-business-label = Nutzen
feedback-business-placeholder = Warum ist das wichtig? Wer ist betroffen?
feedback-acceptance-label = Abnahmekriterien
feedback-acceptance-placeholder = Wann ist es fertig?
feedback-priority-label = Priorität
feedback-priority-low = Niedrig
feedback-priority-medium = Mittel
feedback-priority-high = Hoch

# Spracheingabe — eine Aufnahme füllt alle Felder oben.
feedback-voice-button-label = Per Sprache ausfüllen
feedback-voice-button-title = Antippen, das Problem schildern, erneut antippen — wir füllen die Felder unten aus
feedback-voice-stop-label = Stoppen & ausfüllen
feedback-voice-working-label = Wird transkribiert …
feedback-voice-applied = Aus deiner Aufnahme ausgefüllt — bitte prüfen und senden.
feedback-voice-no-speech = Keine Sprache erkannt — bitte erneut versuchen.

# Screenshot + Annotation.
feedback-shot-label = Screenshot
feedback-shot-status-capturing = Wird aufgenommen …
feedback-shot-status-attached = Angehängt — zum Markieren darauf zeichnen
feedback-shot-status-none = Kein Screenshot
feedback-shot-status-failed = Screenshot nicht verfügbar
feedback-shot-capture = Screenshot hinzufügen
feedback-shot-recapture = Neu aufnehmen
feedback-shot-remove = Entfernen
feedback-shot-exact = Pixelgenau
feedback-shot-exact-title = Nimmt die echten Bildschirmpixel über die Bildschirmfreigabe des Browsers auf — inklusive Canvas, WebGL und eingebetteter Frames, die die normale Aufnahme nicht darstellen kann.
feedback-shot-exact-cancelled = Bildschirmaufnahme abgebrochen — der bisherige Screenshot bleibt erhalten.
feedback-shot-capture-failed = Screenshot konnte nicht aufgenommen werden.

feedback-shot-annotate = Markieren
feedback-annotate-heading = Screenshot markieren
feedback-annotate-done = Zurück zum Formular
feedback-annotate-hint = Zum Zeichnen ziehen. Mit dem Verschieben-Werkzeug (oder gedrückter mittlerer Maustaste) den vergrößerten Ausschnitt bewegen; Strg/⌘ + Scrollen zoomt.
feedback-tool-pan-title = Verschieben
feedback-zoom-preset-title = Klicken: ganzen Screenshot einpassen, nochmal klicken: auf volle Breite
feedback-zoom-fit-label = Einpassen
feedback-zoom-width-label = Breite

feedback-tool-rect-title = Rechteck
feedback-tool-arrow-title = Pfeil
feedback-tool-pen-title = Freihand
feedback-tool-text-title = Text
feedback-tool-redact-title = Verdecken / schwärzen (gefüllter Kasten)
feedback-tool-text-prompt = Text der Anmerkung
feedback-color-aria = Farbe
feedback-undo-title = Rückgängig
feedback-redo-title = Wiederholen
feedback-clear-annot-title = Anmerkungen löschen
feedback-clear-annot-label = Löschen
feedback-zoom-out-title = Verkleinern
feedback-zoom-in-title = Vergrößern

# Zusätzliche Bilder, eingefügt oder auf das Formular gezogen.
feedback-attachments-label = Bilder
feedback-attachments-count = { $count } von { $max }
feedback-attachments-hint = Bilder hier einfügen oder ablegen, um sie anzuhängen.
feedback-attachments-drop = Zum Anhängen ablegen
feedback-attachments-remove = Bild entfernen
feedback-attachments-too-many = Höchstens { $max } Bilder.
feedback-attachments-too-large = Dieses Bild ist größer als { $max } MB.
feedback-attachments-invalid = Es können nur Bilddateien angehängt werden.

# Diagnose-Einwilligung. Beides ist standardmäßig aktiv; das Chat-Protokoll
# erscheint nur auf einer Konversationsseite.
feedback-log-browser-label = Browser-Aktivitätsprotokoll senden (Konsole + Netzwerk)
feedback-log-chat-label = Chat- und Werkzeugprotokoll senden
feedback-diagnostics-label = Anzuhängende Daten anzeigen

feedback-confirm-heading = Bist du sicher?
feedback-confirm-public-p1-prefix = Dieses Feedback erstellt ein Ticket in unserem
feedback-confirm-public-p1-strong = öffentlichen
feedback-confirm-public-p1-suffix = Issue-Tracker. Jede und jeder kann es lesen.
feedback-confirm-private-p2-prefix = Stelle bitte sicher, dass dein Screenshot und die übermittelten Daten
feedback-confirm-private-p2-strong = keine personenbezogenen oder privaten Informationen
feedback-confirm-private-p2-suffix = enthalten (Namen, E-Mail-Adressen, Tokens, Kundendaten …).
feedback-confirm-cancel-button = Nein, ich möchte noch bearbeiten
feedback-confirm-ok-button = Ja, senden

feedback-thanks-heading = Danke
feedback-thanks-body = Dein Feedback wurde als Issue angelegt.
feedback-thanks-issue = Als Issue #{ $number } angelegt.
feedback-thanks-open = Issue öffnen
feedback-done-button = Fertig

feedback-err-no-session = Keine aktive Sitzung
feedback-err-session-lookup-failed = Sitzungssuche fehlgeschlagen
feedback-err-body-read = { $error }
feedback-err-empty-transcript = Leeres Transkript
feedback-err-malformed-json = Ungültiges JSON: { $error }
feedback-err-no-chat-model = Kein Chat-Modell für die Auswertung verfügbar
feedback-err-extraction-failed = Auswertung fehlgeschlagen: { $error }
feedback-err-not-configured = Feedback ist nicht konfiguriert
feedback-err-title-required = Titel ist erforderlich (mindestens 4 Zeichen)
feedback-err-description-required = Beschreibung ist erforderlich
feedback-err-submit-failed = Issue konnte nicht angelegt werden — bitte erneut versuchen
feedback-err-network = Netzwerkfehler: { $error }
