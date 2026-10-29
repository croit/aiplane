# STATUS: llm-generated, unreviewed — pending native-speaker QA

tools-heading = Werkzeuge
tools-description = Schalten Sie die Werkzeuge ein oder aus, die der Assistent verwenden darf. Änderungen gelten nur für Ihr Konto und wirken sich auf Ihre nächste Nachricht aus.
tools-none-granted = Ihre Rollen gewähren keine Werkzeuge.

tools-location-heading = Standort
tools-location-description = Teilen Sie den genauen Standort Ihres Geräts, damit der Assistent Fragen wie „Wie ist das Wetter hier?“ beantworten kann. Er wird nur für Ihre Werkzeugaufrufe verwendet, und Sie können die Freigabe jederzeit beenden. Ohne ihn greift der Assistent auf einen ungefähren, aus Ihrer IP-Adresse abgeleiteten Standort zurück.
tools-location-share-button = Genauen Standort teilen
tools-location-stop-button = Freigabe beenden
tools-location-shared = Freigegeben.
tools-location-shared-accuracy = Freigegeben — Genauigkeit ±{ $accuracy } m.
tools-location-not-shared = Nicht freigegeben.
tools-location-unavailable = Auf Ihren Standort konnte nicht zugegriffen werden. Prüfen Sie die Standortberechtigung Ihres Browsers und versuchen Sie es erneut.

# SPA-only: the Svelte /tools toggle list.
tools-toggle-aria = { $name } umschalten
tools-no-description = Keine Beschreibung

# Section headings for the tool catalog, shared by /tools, the per-token
# capability panel, the chat capability picker and the admin grant matrix.
tool-category-web-network = Web & Netzwerk
tool-category-attachments-documents = Anhänge & Dokumente
tool-category-document-templates = Dokumentvorlagen
tool-category-knowledge-base = Wissensdatenbank
tool-category-code-sandbox = Code & Sandbox
tool-category-memory = Speicher
tool-category-integrations = Integrationen
tool-category-utility = Dienstprogramme
tool-category-skills = Skills
tool-category-images-media = Bilder & Medien
tool-category-comfyui-workflows = ComfyUI-Workflows
tool-category-scheduled-actions = Geplante Aktionen

tools-configure-link = Konfigurieren
tools-needs-storage = Dateispeicher erforderlich.
tools-needs-rag = Wissensdatenbank erforderlich.
tools-needs-push = Push-Benachrichtigungen erforderlich.
tools-needs-geoip = GeoIP-Datenbank erforderlich.
tools-needs-image-backend = Bild-Backend erforderlich.
tools-needs-sandbox = Sandbox-Runner erforderlich.
tools-needs-sandbox-network = Netzwerkzugang für die Sandbox erforderlich.

# SPA-only: /tools/browser, setting up the Chrome extension browser_control drives.
tools-browser-tab = Browser-Erweiterung
tools-browser-description = Lass eine Unterhaltung in diesem Browser handeln, mit deinen Anmeldungen, auf Seiten, die nur du erreichst: ein internes Tool, eine Seite hinter Single Sign-on, ein Formular, das nur du absenden darfst. Das funktioniert nur, solange die Unterhaltung offen ist, und erst, wenn du die Erweiterung einschaltest.
tools-browser-status-heading = Status
tools-browser-status-not-granted = Deine Rollen erlauben keine Browser-Steuerung. Frag deinen Administrator, wenn du sie brauchst.
tools-browser-status-tool-off = Die Browser-Steuerung ist in deiner Tool-Liste ausgeschaltet.
tools-browser-status-not-detected = Auf dieser Seite hat keine Erweiterung geantwortet. Installiere sie und trage dieses AIplane in ihren Einstellungen ein.
tools-browser-status-switched-off = Die Erweiterung ist installiert und gekoppelt, aber ausgeschaltet.
tools-browser-status-ready = Bereit. Der Assistent kann in diesem Browser handeln.
tools-browser-switch-on = Einschalten
tools-browser-switch-on-fallback = Chrome hat das Fenster der Erweiterung nicht geöffnet. Klicke in der Symbolleiste auf das Symbol der Erweiterung und wähle dort Einschalten.
tools-browser-open-tools = Tool-Liste öffnen
tools-browser-recheck = Erneut prüfen
tools-browser-install-heading = Installieren
tools-browser-store-button = Chrome Web Store
tools-browser-download-button = .zip herunterladen
tools-browser-download-note = Die .zip ist zum Installieren als entpackte Erweiterung gedacht: unter Linux oder im Entwicklermodus von Chrome. Unter Windows und macOS installiert Chrome Erweiterungen nur aus dem Store.
tools-browser-steps-heading = Einrichtung
tools-browser-step-install = Installiere die Erweiterung aus dem Chrome Web Store. Sie braucht Chrome 127 oder neuer.
tools-browser-step-pair = Öffne die Einstellungen der Erweiterung und trage die Adresse dieses AIplane ein. Chrome fragt dann nach der Berechtigung für diese Adresse; wenn du sie erlaubst, ist die Kopplung fertig.
tools-browser-step-switch-on = Komm auf diese Seite zurück und klicke auf Einschalten, oder nutze das Symbol der Erweiterung in der Symbolleiste. Beim ersten Mal fragt Chrome nach Zugriff auf Websites.
tools-browser-step-ask = Frag in einer Unterhaltung, zum Beispiel: „Öffne unser internes Wiki und fasse die Onboarding-Seite zusammen.“
tools-browser-unpacked-heading = Entpackt installieren
tools-browser-unpacked-steps = Entpacke den Download, öffne chrome://extensions, schalte den Entwicklermodus ein, klicke auf „Entpackte Erweiterung laden“ und wähle den entpackten Ordner.
tools-browser-origin-label = Adresse dieses AIplane
tools-browser-copy-origin = Kopieren
tools-browser-copied = Kopiert
tools-browser-notes-heading = Gut zu wissen
tools-browser-note-window = Der Assistent arbeitet in einem eigenen Fenster, in einer Tab-Gruppe namens „Assistant“. Solange die Erweiterung an ist, ist ihr Symbol grün und Chrome zeigt eine Leiste, dass der Browser debuggt wird.
tools-browser-note-open = Es funktioniert nur, solange die Unterhaltung in einem Tab offen ist. Schalte die Erweiterung über ihr Symbol aus, wenn du fertig bist.
tools-browser-note-injection = Seiten, die der Assistent liest, können Text enthalten, der sich an ihn richtet. In den Einstellungen der Erweiterung kannst du sie auf Seiten beschränken, die du freigibst.

tool-forward-request-title = Anderen Agenten fragen
tool-forward-request-description = Sendet eine ausdrückliche Aufgabe an einen anderen persönlichen Agenten. Beide Agenten müssen die Anfrage erlauben; der empfangende Agent verwendet seine eigenen Werkzeuge und Freigaberegeln.
