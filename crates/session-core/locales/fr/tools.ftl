# STATUS: llm-generated, unreviewed — pending native-speaker QA

tools-heading = Outils
tools-description = Activez ou désactivez les outils que l'assistant peut utiliser. Les modifications ne s'appliquent qu'à votre compte et prennent effet à votre prochain message.
tools-none-granted = Vos rôles n'accordent aucun outil.

tools-location-heading = Localisation
tools-location-description = Partagez la position précise de votre appareil afin que l'assistant puisse répondre à des questions comme « quel temps fait-il ici ? ». Elle n'est utilisée que pour vos appels d'outils et vous pouvez arrêter le partage à tout moment. Sans elle, l'assistant se rabat sur une position approximative dérivée de votre adresse IP.
tools-location-share-button = Partager la position précise
tools-location-stop-button = Arrêter le partage
tools-location-shared = Partagée.
tools-location-shared-accuracy = Partagée — précision ±{ $accuracy } m.
tools-location-not-shared = Non partagée.
tools-location-unavailable = Impossible d'accéder à votre position. Vérifiez l'autorisation de localisation du navigateur et réessayez.

# SPA-only: the Svelte /tools toggle list.
tools-toggle-aria = Activer/désactiver { $name }
tools-no-description = Aucune description

# Section headings for the tool catalog, shared by /tools, the per-token
# capability panel, the chat capability picker and the admin grant matrix.
tool-category-web-network = Web & Réseau
tool-category-attachments-documents = Pièces jointes & Documents
tool-category-document-templates = Modèles de documents
tool-category-knowledge-base = Base de connaissances
tool-category-code-sandbox = Code & Bac à sable
tool-category-memory = Mémoire
tool-category-integrations = Intégrations
tool-category-utility = Utilitaires
tool-category-skills = Skills
tool-category-images-media = Images & Médias
tool-category-comfyui-workflows = Workflows ComfyUI
tool-category-scheduled-actions = Actions planifiées

tools-configure-link = Configurer
tools-needs-storage = Stockage de fichiers requis.
tools-needs-rag = Base de connaissances requise.
tools-needs-push = Notifications push requises.
tools-needs-geoip = Base de données GeoIP requise.
tools-needs-image-backend = Backend image requis.
tools-needs-sandbox = Exécuteur sandbox requis.
tools-needs-sandbox-network = Accès réseau requis pour la sandbox.

# SPA-only: /tools/browser, setting up the Chrome extension browser_control drives.
tools-browser-tab = Extension de navigateur
tools-browser-description = Permettez à une conversation d'agir dans ce navigateur, avec vos connexions, sur des pages que vous seul pouvez atteindre : un outil interne, une page derrière une authentification unique, un formulaire que vous seul pouvez envoyer. Cela ne fonctionne que tant que la conversation est ouverte, et seulement après avoir activé l'extension.
tools-browser-status-heading = État
tools-browser-status-not-granted = Vos rôles n'autorisent pas le contrôle du navigateur. Demandez à votre administrateur si vous en avez besoin.
tools-browser-status-tool-off = Le contrôle du navigateur est désactivé dans votre liste d'outils.
tools-browser-status-not-detected = Aucune extension n'a répondu sur cette page. Installez-la et ajoutez cet AIplane dans ses paramètres.
tools-browser-status-switched-off = L'extension est installée et associée, mais désactivée.
tools-browser-status-ready = Prêt. L'assistant peut agir dans ce navigateur.
tools-browser-switch-on = Activer
tools-browser-switch-on-fallback = Chrome n'a pas ouvert la fenêtre de l'extension. Cliquez sur l'icône de l'extension dans la barre d'outils et choisissez Activer.
tools-browser-open-tools = Ouvrir la liste des outils
tools-browser-recheck = Vérifier à nouveau
tools-browser-install-heading = Installer
tools-browser-store-button = Chrome Web Store
tools-browser-download-button = Télécharger le .zip
tools-browser-download-note = Le .zip sert à installer l'extension non empaquetée : sous Linux, ou en mode développeur de Chrome. Sous Windows et macOS, Chrome n'installe des extensions que depuis le store.
tools-browser-steps-heading = Configuration
tools-browser-step-install = Installez l'extension depuis le Chrome Web Store. Elle nécessite Chrome 127 ou plus récent.
tools-browser-step-pair = Ouvrez les paramètres de l'extension et ajoutez l'adresse de cet AIplane. Chrome demande alors l'autorisation pour cette adresse ; l'accepter termine l'association.
tools-browser-step-switch-on = Revenez sur cette page et cliquez sur Activer, ou utilisez l'icône de l'extension dans la barre d'outils. La première fois, Chrome demande l'accès aux sites web.
tools-browser-step-ask = Demandez dans une conversation, par exemple : « Ouvre notre wiki interne et résume la page d'intégration. »
tools-browser-unpacked-heading = Installer non empaquetée
tools-browser-unpacked-steps = Décompressez le téléchargement, ouvrez chrome://extensions, activez le mode développeur, cliquez sur « Charger l'extension non empaquetée » et choisissez le dossier décompressé.
tools-browser-origin-label = Adresse de cet AIplane
tools-browser-copy-origin = Copier
tools-browser-copied = Copié
tools-browser-notes-heading = Bon à savoir
tools-browser-note-window = L'assistant travaille dans sa propre fenêtre, dans un groupe d'onglets nommé « Assistant ». Tant que l'extension est active, son icône est verte et Chrome affiche une barre indiquant que le navigateur est en cours de débogage.
tools-browser-note-open = Cela ne fonctionne que tant que la conversation est ouverte dans un onglet. Désactivez l'extension depuis son icône quand vous avez terminé.
tools-browser-note-injection = Les pages lues par l'assistant peuvent contenir du texte qui lui est destiné. Dans les paramètres de l'extension, vous pouvez la limiter aux sites que vous approuvez.

tool-forward-request-title = Demander à un autre agent
tool-forward-request-description = Envoie une tâche explicite à un autre agent personnel. Les deux agents doivent autoriser la demande ; l’agent destinataire utilise ses propres outils et règles d’approbation.
