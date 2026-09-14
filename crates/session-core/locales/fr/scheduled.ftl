# STATUS: llm-generated, unreviewed — pending native-speaker QA


scheduled-heading = Actions planifiées
scheduled-intro = Exécutez automatiquement un prompt selon un planning. Chaque exécution ouvre une nouvelle conversation que vous pouvez consulter ici — choisissez un modèle, écrivez le prompt et définissez quand il doit s'exécuter.
scheduled-create-submit = Créer une action planifiée
scheduled-list-heading = Vos actions planifiées
scheduled-list-empty = Aucune action planifiée pour l'instant. Créez-en une avec le bouton ci-dessus.

scheduled-back = Retour
scheduled-edit-heading = Modifier l'action planifiée
scheduled-save-submit = Enregistrer les modifications

scheduled-name-label = Nom
scheduled-name-placeholder = ex. Résumé quotidien de l'actualité
scheduled-model-label = Modèle
scheduled-model-placeholder = identifiant du modèle (ex. gpt-4o-mini)
scheduled-gdpr-warning = Ce modèle n'est pas conforme au RGPD. Les exécutions planifiées lui enverront automatiquement votre prompt — évitez les données personnelles.
scheduled-nda-warning = Ce modèle n'est pas couvert par un accord de confidentialité. Ne planifiez pas de contenu protégé par NDA ou propriétaire vers ce modèle.
scheduled-prompt-label = Prompt
scheduled-prompt-placeholder = Que doit faire le modèle à chaque exécution ?
scheduled-tools-toggle-label = Autoriser les outils (recherche web, RAG, pièces jointes) — comme dans le chat
scheduled-reuse-toggle-label = Réutiliser la conversation de l'exécution précédente — chaque exécution poursuit la même conversation
scheduled-reuse-rounds-prefix = envoyer les
scheduled-reuse-rounds-aria = Nombre de tours d'historique à rejouer
scheduled-reuse-rounds-suffix = derniers tours

scheduled-builder-heading = Planning
scheduled-mode-hourly = Toutes les heures
scheduled-mode-daily = Quotidien
scheduled-mode-weekly = Hebdomadaire
scheduled-mode-monthly = Mensuel
scheduled-mode-advanced = Avancé
scheduled-weekday-mon = Lun
scheduled-weekday-tue = Mar
scheduled-weekday-wed = Mer
scheduled-weekday-thu = Jeu
scheduled-weekday-fri = Ven
scheduled-weekday-sat = Sam
scheduled-weekday-sun = Dim
scheduled-on-day-label = Le jour
scheduled-of-every-month = de chaque mois
scheduled-at-label = À
scheduled-hour-aria = Heure
scheduled-minute-aria = Minute
scheduled-of-every-hour = de chaque heure
scheduled-timezone-label = Fuseau horaire
scheduled-cron-label = Expression cron
scheduled-cron-help = Cinq champs : minute heure jour-du-mois mois jour-de-semaine.

scheduled-no-upcoming-runs = Aucune exécution à venir.
scheduled-next-runs-prefix = Prochaines exécutions :{ " " }

scheduled-err-pick-weekday = Choisissez au moins un jour de la semaine.
scheduled-err-enter-cron = Saisissez une expression cron.


scheduled-toast-not-found = Aucune action planifiée de ce type.

scheduled-badge-active = active
scheduled-badge-paused = suspendue
scheduled-status-paused = Suspendue
scheduled-next-run = Prochaine exécution : { $when }
scheduled-no-upcoming-run = Aucune exécution à venir
scheduled-last-success = Dernière : ✓ { $when }
scheduled-last-failure = Dernière : ✗ { $when }
scheduled-pause-title = Suspendre
scheduled-resume-title = Reprendre
scheduled-edit-title = Modifier
scheduled-delete-title = Supprimer
scheduled-delete-confirm = Supprimer cette action planifiée ?
scheduled-preview-summary = { $summary } ({ $timezone })
scheduled-preview-next-runs = Prochaines exécutions : { $runs }
scheduled-last-error = Dernière erreur : { $error }

# Les liens de la page de liste vers ce qu'une planification a produit, et
# l'historique des exécutions derrière eux (`/scheduled/{id}/runs`).
scheduled-create-heading = Nouvelle action planifiée
scheduled-new-page-title = Nouvelle action planifiée
scheduled-edit-named-heading = Modifier { $name }
scheduled-toast-created = Action planifiée créée.
scheduled-toast-saved = Action planifiée enregistrée.
scheduled-badge-reuses-chat = une seule conversation
scheduled-open-chat = Ouvrir le chat
scheduled-open-chats = { $count ->
    [one] { $count } chat
   *[other] { $count } chats
}
scheduled-open-runs = { $count ->
    [one] { $count } exécution
   *[other] { $count } exécutions
}
scheduled-never-run = Jamais exécutée
scheduled-runs-page-title = Historique des exécutions
scheduled-runs-heading = Exécutions · { $name }
scheduled-runs-intro = Chaque exécution enregistrée, la plus récente en premier, avec le chat qu'elle a ouvert.
scheduled-runs-empty = Aucune exécution pour l'instant. Cette action ne s'est pas déclenchée depuis sa création.
scheduled-run-open = ouvrir le chat
scheduled-run-status-ok = ok
scheduled-run-status-error = erreur
scheduled-run-status-pending = en cours
