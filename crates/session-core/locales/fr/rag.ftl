# STATUS: llm-generated, unreviewed — pending native-speaker QA

rag-heading = Collections RAG
rag-description-prefix = Bases de code que la passerelle a indexées. L'outil
rag-description-suffix = interroge ces collections pour répondre aux questions sur le code.
rag-collections-heading = Collections configurées
rag-empty-list = Aucune collection pour l'instant. Créez-en une ci-dessus.

# Toasts — collection CRUD
rag-toast-indexing-queued = L'indexation de `{ $name }` @ `{ $ref }` a été mise en file d'attente.
rag-toast-created-aggregate = `{ $name }` créée (agrégat). Ajoutez les dépôts sources ci-dessous pour les indexer.
rag-toast-collection-saved = `{ $name }` enregistrée.
rag-toast-vanished = La collection a disparu après l'enregistrement.
# Toasts — refs / sources
rag-toast-bulk-queued-skipped = { $added } source(s) mise(s) en file d'attente ; { $skipped } doublon(s) ignoré(s).
rag-toast-bulk-queued = Indexation de { $added } source(s) mise en file d'attente.
rag-toast-source-updated = Source mise à jour.

# Status badges
rag-status-pending = en attente
rag-status-unconfigured = aucune source
rag-status-cloning = clonage
rag-status-indexing = indexation
rag-status-ready = prêt
rag-status-error = erreur

# Collection row
rag-pat-set = PAT défini
rag-pat-none = pas de PAT
rag-meta-aggregate = { $count } source(s) · { $hint }
rag-meta-versioned = { $url } · { $hint }
rag-badge-aggregate = agrégat
rag-embed-prefix = embed :
rag-button-edit = Modifier
rag-button-delete-collection = Supprimer la collection
rag-placeholder-source-git-url = https://github.com/org/repo.git
rag-button-add-source = Ajouter une source
rag-placeholder-branch-tag-commit = branche, tag ou commit
rag-button-add-ref = Ajouter une référence
rag-placeholder-bulk-sources = Ajout en masse — un dépôt par ligne, @ref facultatif :
    https://github.com/proxmox/pve-manager.git
    https://github.com/proxmox/qemu-server.git @master
rag-button-add-bulk = Ajouter des sources (en masse)

# Ref / source row
rag-badge-primary = principale
rag-ref-indexed-line = indexé { $date } · { $commit }
rag-never = jamais
rag-button-log = Journal
rag-button-reindex = Réindexer
rag-button-set-primary = Définir comme principale
rag-button-remove = Supprimer

# Indexing log
rag-log-info = info
rag-log-warn = avertissement
rag-log-error = erreur
rag-log-heading = Journal d'indexation
rag-log-empty = Aucun événement d'indexation enregistré pour l'instant. La première exécution s'enregistrera ici dès que l'indexeur traitera cette référence.

# Inline per-source editor
rag-label-git-url-source = URL Git (cette source)
rag-label-git-url-inherit = URL Git (vide = hériter de la collection)
rag-placeholder-git-url = https://example.com/org/repo.git
rag-label-branch-tag = Branche / tag
rag-button-save-source = Enregistrer la source
rag-button-cancel = Annuler

# Create-collection form
rag-create-heading = Indexer une nouvelle collection
rag-create-description = L'indexeur clone le dépôt, découpe chaque fichier en chunks et les embedde via le modèle d'embedding configuré. Les PAT sont stockés en clair (la passerelle s'exécute sur une infrastructure de confiance).
rag-new-page-title = Indexer une nouvelle collection
rag-edit-page-title = Modifier la collection
rag-not-found = Aucune collection avec cet identifiant. Elle a peut-être été supprimée.
rag-back-to-collections = Collections RAG
rag-edit-source-heading = Modifier la source
rag-add-source-heading = Ajouter une source
rag-label-name = Nom
rag-placeholder-name = ex. gateway-repo
rag-label-description-optional = Description (facultatif)
rag-placeholder-description = courte, lisible
rag-label-git-url-versioned = URL Git (versionné uniquement)
rag-label-pat-optional = Jeton d'accès personnel (facultatif)
rag-placeholder-pat = pour les dépôts privés
rag-label-include-globs-full = Inclusions (globs séparés par des virgules ou des retours à la ligne)
rag-placeholder-include-globs = *.rs, *.md
rag-label-exclude-globs = Exclusions (globs)
rag-placeholder-exclude-globs = target/, node_modules/
rag-label-chunk-size = Taille du chunk
rag-label-chunk-overlap = Chevauchement du chunk
rag-label-refresh-interval = Resynchronisation automatique
rag-hint-refresh-interval = À quelle fréquence réindexer sans que personne le demande. Une source qui ne peut pas prévenir la passerelle — une archive de liste de diffusion, un partage WebDAV simple — n'est à jour que dans cette mesure.
rag-refresh-never = Jamais (manuel ou hook de synchronisation)
rag-refresh-hourly = Toutes les heures
rag-refresh-daily = Tous les jours
rag-refresh-weekly = Toutes les semaines
rag-refresh-custom = Toutes les { $mins } minutes
rag-create-aggregate-help = Agrégat (multi-source) : recherche dans de nombreux dépôts comme un seul corpus. Laissez l'URL Git vide et ajoutez chaque dépôt source après la création. La branche / le tag devient la référence par défaut des sources ajoutées.
rag-button-queue-indexing = Planifier l'indexation

# Edit-collection form
rag-edit-heading = Modification de { $name }
rag-label-description = Description
rag-label-pat = Jeton d'accès personnel
rag-placeholder-pat-keep = laisser vide pour conserver l'existant
rag-label-clear-pat = Supprimer le PAT enregistré (ne plus s'authentifier)
rag-label-include-globs = Inclusions (globs)
rag-button-save-changes = Enregistrer les modifications

# Embedding model field
rag-label-embedding-model = Modèle d'embedding
rag-placeholder-embedding-model-none = aucun pool d'embedding configuré — saisissez un identifiant de modèle
rag-option-choose-embedding-model = Choisir un modèle d'embedding…
rag-suffix-not-advertised = (plus proposé)

rag-label-allowed-groups = Groupes autorisés
rag-hint-allowed-groups = Groupes AIplane autorisés à lister et rechercher cette collection. Aucune sélection = tout le monde disposant des outils RAG. Les admins ont toujours accès.

# Sélecteur de source + identifiants du fournisseur (rag_source.rs). Les
# libellés des champs viennent du fournisseur et ne sont pas traduits.
rag-label-source-kind = Source
rag-source-git-help = Clone un dépôt et indexe ses fichiers. Le comportement d'origine.
rag-source-secret-placeholder = laisser vide pour conserver la valeur enregistrée
rag-source-unknown-kind = Type de source inconnu.
rag-source-test-button = Tester la connexion
rag-source-test-ok = Connecté en tant que `{ $account }`. { $entries } élément(s) dans le dossier configuré.
rag-source-test-ok-plain = Connecté. { $entries } élément(s) dans le dossier configuré.
rag-source-test-failed = Source injoignable : { $error }
rag-source-test-git = Choisissez une source distante à tester. Les dépôts Git sont vérifiés lors de l'indexation.
rag-source-detected = Détecté : { $server }

rag-label-profile = Champs de document
rag-option-profile-none = Aucun — indexer seulement le texte
rag-profile-help = Extrait des champs (fournisseur, date, montant, projet) de chaque document afin de pouvoir les filtrer, trier et totaliser. Coûte un appel de modèle par document ; laissez « Aucun » pour du code ou du texte brut.

# Éditeur de profils d'extraction (/rag/profiles, rag_profiles.rs)
rag-profile-heading = Profils d'extraction
rag-profile-description = Ce qui est extrait de chaque document d'une collection : les champs qui rendent « la dernière facture de X » ou « combien avons-nous dépensé » réellement répondables. Un profil s'attache à une collection depuis la page RAG.
rag-profile-create-heading = Nouveau profil
rag-profile-list-heading = Profils
rag-profile-empty = Aucun profil pour l'instant.
rag-profile-builtin = fourni
rag-profile-version = v{ $version }
rag-profile-summary = { $count } champ(s)
rag-profile-label-name = Nom
rag-profile-label-description = Description
rag-profile-label-prompt = Instructions d'extraction
rag-profile-label-fields = Champs (JSON)
rag-profile-prompt-placeholder = Décrivez ce que le modèle lit et comment normaliser les dates et les montants.
rag-profile-fields-help = Un objet par champ : key, label, type (text | number | date | enum), description, et facultativement filterable / sortable. Un enum exige aussi « values ». La description est transmise au modèle : soyez précis.
rag-profile-edit-warning = L'enregistrement incrémente la version du profil et vide son cache d'extraction. Les collections qui l'utilisent doivent être ré-indexées pour reprendre les nouveaux champs.
rag-profile-button-create = Créer le profil
rag-profile-button-save = Enregistrer
rag-profile-button-delete = Supprimer
rag-profile-delete-confirm = Supprimer le profil { $name } ?
rag-profile-example-counterparty-label = Contrepartie
rag-profile-example-counterparty-description = L'autre partie.
rag-profile-example-date-label = Date
rag-profile-example-date-description = La date du document.
rag-profile-example-amount-label = Montant
rag-profile-example-amount-description = Le montant total.
rag-profile-link = Modifier les profils d'extraction
rag-profile-toast-created = Profil « { $name } » créé.
rag-profile-toast-saved = « { $name } » enregistré.
rag-profile-toast-saved-reindex = « { $name } » enregistré. Ré-indexez pour l'appliquer : { $collections }.
rag-profile-toast-deleted = Profil supprimé.
# Hook de synchronisation — un déclencheur entrant qui resynchronise une collection.
rag-toast-sync-token = URL de synchronisation (affichée une seule fois, non stockée) : { $url }
rag-toast-sync-token-cleared = URL de synchronisation désactivée.
rag-button-sync-token = URL de sync
rag-button-sync-token-rotate = Nouvelle URL de sync
rag-button-sync-token-clear = Désactiver l'URL de sync
rag-badge-sync-hook = hook de sync

# Browser consent for an OAuth source (Google Drive).
rag-source-consent-save-first = Enregistrez d'abord la collection avec son ID client et son secret, puis connectez-la pour accorder l'accès.
rag-source-consent-connected = connectée
rag-source-consent-connect = Connecter
rag-oauth-lookup-failed = Impossible de lire la collection.
rag-oauth-not-oauth = Ce type de source ne se connecte pas dans le navigateur.
rag-oauth-no-client = Enregistrez d'abord l'ID client et le secret OAuth sur la collection.
rag-oauth-bad-authorize-url = Impossible de construire l'URL d'autorisation du fournisseur.
rag-oauth-start-failed = Impossible de démarrer l'autorisation.
rag-oauth-callback-missing = Le code ou l'état manquait dans la réponse du fournisseur.
rag-oauth-expired = Cette autorisation a expiré ou a déjà été utilisée. Recommencez.
rag-oauth-provider-refused = Le fournisseur a refusé l'autorisation : { $error }
rag-oauth-exchange-failed = L'échange du code d'autorisation a échoué : { $error }
rag-oauth-no-refresh-token = Le fournisseur n'a renvoyé aucun jeton de rafraîchissement ; l'indexation autonome serait impossible. Révoquez l'accès de la passerelle dans votre compte fournisseur puis reconnectez.
rag-oauth-store-failed = Impossible d'enregistrer les identifiants.
rag-badge-no-files = aucun fichier indexé
rag-ref-files = { $files } fichiers
rag-label-git-url = URL Git
rag-source-testing = Test en cours…
rag-sync-url-heading = URL de synchronisation — affichée une seule fois
rag-sync-token-confirm = Générer une nouvelle URL de synchronisation ? L'ancienne cessera de fonctionner.
rag-delete-collection-confirm = Supprimer la collection { $name } et son index ?
rag-remove-source-confirm = Retirer la source { $source } ?
rag-toast-rebuild-queued = Reconstruction complète demandée.
rag-no-sources = Aucune source — cette collection n'indexe rien tant qu'aucune n'est ajoutée.
rag-add-sources-hint = Une source par ligne ; ajoutez { $at } pour remplacer la réf. { $ref } de cette collection.
