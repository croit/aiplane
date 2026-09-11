# STATUS: llm-generated, unreviewed — pending native-speaker QA


webhooks-heading = Webhooks
webhooks-intro = Exécutez un prompt lorsqu'un service externe appelle une URL. Vous obtenez une URL de déclenchement secrète ; ce que l'appelant envoie dans le corps de la requête est ajouté à votre prompt, et l'exécution s'ouvre comme une nouvelle conversation que vous pouvez lire ici.
webhooks-create-submit = Créer un webhook
webhooks-save-submit = Enregistrer les modifications
webhooks-edit-heading = Modifier le webhook
webhooks-back = Retour
webhooks-list-heading = Vos webhooks
webhooks-list-empty = Aucun webhook pour l'instant. Créez-en un ci-dessus.

webhooks-name-label = Nom
webhooks-name-placeholder = p. ex. Résumé de déploiement
webhooks-model-label = Modèle
webhooks-model-placeholder = Identifiant du modèle
webhooks-prompt-label = Prompt
webhooks-prompt-placeholder = Que doit faire le modèle avec les données reçues ?

webhooks-sync-toggle-label = Attendre la réponse (renvoyer la sortie du modèle à l'appelant)
webhooks-tools-toggle-label = Autoriser les outils (exécuter avec vos outils, p. ex. recherche web, RAG, connecteurs)
webhooks-tools-warning = Toute personne disposant de l'URL de déclenchement peut envoyer du contenu que le modèle traite avec vos outils, en votre nom. N'activez ceci que pour un appelant de confiance.

webhooks-gdpr-warning = Ce modèle s'exécute hors de l'UE. N'envoyez pas de données personnelles via ce webhook.
webhooks-nda-warning = Ce modèle n'est pas autorisé pour du contenu sous NDA. N'envoyez pas de données confidentielles via ce webhook.

webhooks-reveal-heading = Votre URL de déclenchement
webhooks-reveal-note = Copiez-la maintenant — elle n'est affichée qu'une seule fois. Quiconque possède cette URL peut déclencher le webhook. Perdue ? Effectuez une rotation pour en obtenir une nouvelle.
webhooks-copy = Copier

webhooks-badge-active = Actif
webhooks-badge-paused = En pause
webhooks-mode-sync = Attend la réponse
webhooks-mode-async = Sans attente
webhooks-never-fired = Jamais déclenché
webhooks-last-success = Dernier déclenchement { $when }
webhooks-last-success-open = Dernier déclenchement { $when } — ouvrir
webhooks-last-failure = Dernier déclenchement échoué { $when }
webhooks-last-failure-open = Dernier déclenchement échoué { $when } — ouvrir

webhooks-pause-title = Mettre en pause
webhooks-resume-title = Reprendre
webhooks-rotate-title = Régénérer le secret
webhooks-edit-title = Modifier
webhooks-delete-title = Supprimer


webhooks-toast-not-found = Webhook introuvable.

# --- Relancer avec un prompt différent ---
webhooks-rerun-link = relancer
webhooks-rerun-heading = Relancer avec un prompt différent
webhooks-rerun-page-name = Relancer le webhook
webhooks-rerun-intro = Rejouez la dernière charge utile reçue par ce webhook, avec un prompt modifiable. L'exécution s'ouvre comme une nouvelle conversation.
webhooks-rerun-payload-label = Charge utile capturée (rejouée telle quelle)
webhooks-rerun-submit = Relancer
webhooks-rerun-no-payload = Ce webhook n'a pas encore capturé de charge utile — déclenchez-le une fois d'abord.
webhooks-rerun-no-payload-notice = Ce webhook n'a pas encore été déclenché, il n'y a donc aucune charge utile à rejouer. Déclenchez-le une fois, puis revenez pour le relancer avec un autre prompt.

# --- Historique des exécutions ---
webhooks-runs-link = exécutions
webhooks-runs-heading = Exécutions · { $name }
webhooks-runs-page-name = Exécutions du webhook
webhooks-runs-intro = Les derniers déclenchements et relances. Ouvrez une exécution pour lire sa conversation, ou relancez sa charge utile avec un autre prompt.
webhooks-runs-empty = Aucune exécution pour l'instant. Déclenchez le webhook pour voir son historique ici.
webhooks-run-open = ouvrir le chat
webhooks-run-rerun = relancer
webhooks-run-source-fire = déclenché
webhooks-run-source-rerun = relance
webhooks-run-status-ok = ok
webhooks-run-status-error = erreur
webhooks-run-status-pending = en cours

# --- Réutilisation de la conversation ---
webhooks-reuse-toggle-label = Réutiliser la conversation (chaque déclenchement poursuit le chat précédent)
webhooks-reuse-rounds-prefix = en rejouant les
webhooks-reuse-rounds-suffix = derniers tours
webhooks-reuse-rounds-aria = Tours d'historique à rejouer

webhooks-toast-rerun-failed = Réexécution { $status }
webhooks-rotate-confirm = Émettre un nouveau secret de déclenchement ? L'ancienne URL cesse de fonctionner immédiatement.
webhooks-delete-confirm = Supprimer ce webhook ?
