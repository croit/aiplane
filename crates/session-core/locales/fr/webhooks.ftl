# STATUS: llm-generated, unreviewed — pending native-speaker QA

webhooks-heading = Webhooks
webhooks-intro = Exécutez un prompt lorsqu'un service externe appelle une URL. Vous obtenez une URL de déclenchement secrète ; ce que l'appelant envoie dans le corps de la requête est ajouté à votre prompt, et l'exécution s'ouvre comme une nouvelle conversation que vous pouvez lire ici.
webhooks-edit-heading = Modifier le webhook
webhooks-list-empty = Aucun webhook pour l'instant. Créez-en un ci-dessus.

webhooks-name-label = Nom
webhooks-name-placeholder = p. ex. Résumé de déploiement
webhooks-model-label = Modèle
webhooks-model-placeholder = Identifiant du modèle
webhooks-prompt-placeholder = Que doit faire le modèle avec les données reçues ?

webhooks-reveal-heading = Votre URL de déclenchement
webhooks-reveal-note = Copiez-la maintenant — elle n'est affichée qu'une seule fois. Quiconque possède cette URL peut déclencher le webhook. Perdue ? Effectuez une rotation pour en obtenir une nouvelle.
webhooks-copy = Copier

webhooks-badge-active = Actif
webhooks-badge-paused = En pause
webhooks-mode-sync = Attend la réponse

webhooks-pause-title = Mettre en pause
webhooks-resume-title = Reprendre
webhooks-rotate-title = Régénérer le secret
webhooks-edit-title = Modifier
webhooks-delete-title = Supprimer

# --- Relancer avec un prompt différent ---
webhooks-toast-rerun-started = Relance terminée — ouverture de la conversation…

# --- Historique des exécutions ---
webhooks-runs-empty = Aucune exécution pour l'instant. Déclenchez le webhook pour voir son historique ici.
webhooks-run-open = ouvrir le chat
webhooks-run-rerun = relancer

# SPA-only: the Svelte /webhooks page — inline form, run list, rerun composer.
webhooks-new-heading = Nouveau webhook
webhooks-prompt-untrusted-label = Prompt (la charge utile arrive comme entrée non fiable)
webhooks-runs-show = Exécutions
webhooks-runs-hide = Masquer les exécutions
webhooks-rerun-prompt-label = Prompt de réexécution — la charge utile enregistrée est rejouée à travers celui-ci
webhooks-rerun-latest = Réexécuter la dernière charge utile
webhooks-rerun-running = En cours…
webhooks-toast-rerun-failed = Réexécution { $status }
webhooks-rotate-confirm = Émettre un nouveau secret de déclenchement ? L'ancienne URL cesse de fonctionner immédiatement.
webhooks-delete-confirm = Supprimer ce webhook ?
