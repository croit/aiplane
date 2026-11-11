# STATUS: llm-generated, unreviewed — pending native-speaker QA


pools-field-name = Nom
pools-field-kind = Type
pools-field-models = Modèles servis (séparés par des virgules)
pools-field-backends = Backends
pools-save-pool = Enregistrer le pool
pools-delete-pool = Supprimer
pools-delete-confirm = Supprimer le pool « { $name } » ? Ses backends restent et deviennent non attribués.

# Cartes de pool de la SPA : les deux résumés d'une ligne et la suppression.
pools-add-heading = Ajouter un pool
pools-fallbacks-heading = Replis pour modèles inconnus
pools-fallbacks-description = Lorsqu'une requête nomme un modèle que la gateway n'a jamais rencontré, ce modèle est substitué pour ce type. Vide = l'absence renvoie 404.
upstreams-problems-heading = Ce pool ne fonctionne pas complètement
pools-name-taken = Un pool portant ce nom existe déjà — enregistrer le remplacerait, backends et modèles compris. Choisissez un autre nom pour créer un nouveau pool.
pools-field-strategy = Stratégie
pools-field-strategy-hint = prefix_affinity garde une conversation sur la réplique qui détient déjà son cache KV (idéal pour le trafic chat/agent réparti sur plusieurs GPU — les autres alternent les tours entre répliques et repaient un prefill complet chaque fois), tout en répartissant si un backend est réellement plus chargé. least_inflight équilibre selon la charge actuelle ; round_robin tourne selon le poids.
pools-field-fallback-offline = Modèle de repli hors ligne
pools-field-fallback-offline-placeholder = servi lorsque tous les backends sont hors service
pools-field-models-hint = Si défini, exactement ces ids sont servis et les backends ne sont pas interrogés sur ce qu'ils proposent — nécessaire lorsque /models liste autre chose ou omet un modèle. Vide = découvrir les modèles via le /models de chaque backend.
pools-field-models-chips = Annoncés par les backends de ce pool — cliquez pour épingler ou retirer
pools-field-allowed-groups = Groupes autorisés
pools-field-allowed-groups-hint = Groupes AIplane autorisés à voir et utiliser les modèles de ce pool. Aucune sélection = tout le monde. Les admins ont toujours accès. Gérez les groupes dans Admin → Groupes.
pools-field-voices = Voix (lang=voice par ligne)
pools-field-offer-voices = Voix sélectionnables (une par ligne, au choix de l'utilisateur)
pools-no-backends = Aucun backend défini pour l'instant. Ajoutez-en un d'abord sur la page Backends.
pools-field-enforce-limits = Appliquer les limites de débit et les quotas

upstreams-problem-no-backends = Aucun backend assigné — rien dans ce pool ne peut servir une requête.
upstreams-problem-all-drained = Tous les backends sont vidés pour maintenance : rien n'est routé ici.
upstreams-problem-all-down = Aucun backend disponible : les requêtes attendent un retour puis reçoivent une erreur de panne temporaire.
upstreams-problem-auth = Identifiants refusés par : { $backends }. La découverte des modèles y est désactivée, ils n'annoncent rien.
upstreams-problem-no-models = Aucun modèle annoncé par : { $backends }. Rien n'y est routé, et un alias nu n'y a rien à quoi se lier.
upstreams-problem-broken-aliases = Alias qui ne routent nulle part : { $aliases }. Chacun pointe vers un modèle que son backend ne sert pas, ou n'a rien à quoi se lier.
upstreams-problem-partial-coverage = Servis par une partie seulement du pool : { $models }. Les requêtes sur ces noms utilisent moins de répliques que vous n'en avez.
upstreams-problem-unserved-allowlist = Listés sous Modèles mais servis par personne : { $models }.
upstreams-problem-missing-backends = Backends assignés qui n'existent plus : { $backends }.
