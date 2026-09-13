# Le widget de retour : le bouton flottant, la boîte de dialogue (formulaire,
# saisie vocale, annotation de capture, pièces jointes, consentement aux
# diagnostics), la confirmation « suivi public » et les erreurs
# `/api/v0/feedback*`.

feedback-fab-aria = Envoyer un retour
feedback-fab-title = Envoyer un retour

feedback-dialog-heading = Envoyer un retour
feedback-dialog-blurb = La page, votre navigateur et l'activité récente de la console et du réseau sont joints automatiquement.
feedback-close-aria = Fermer
feedback-cancel-button = Annuler
feedback-submit-button = Envoyer le retour
feedback-sending = Envoi en cours…

feedback-title-label = Titre
feedback-title-placeholder = Résumé court
feedback-description-label = Description
feedback-description-placeholder = Que s'est-il passé, ou que souhaitez-vous ?
feedback-business-label = Valeur métier
feedback-business-placeholder = Pourquoi est-ce important ? Qui est concerné ?
feedback-acceptance-label = Critères d'acceptation
feedback-acceptance-placeholder = Quand est-ce terminé ?
feedback-priority-label = Priorité
feedback-priority-low = Basse
feedback-priority-medium = Moyenne
feedback-priority-high = Haute

# Saisie vocale — un enregistrement remplit tous les champs ci-dessus.
feedback-voice-button-label = Remplir à la voix
feedback-voice-button-title = Touchez, décrivez le problème, touchez à nouveau — nous remplirons les champs ci-dessous
feedback-voice-stop-label = Arrêter et remplir
feedback-voice-working-label = Transcription…
feedback-voice-applied = Rempli à partir de votre enregistrement — relisez puis envoyez.
feedback-voice-no-speech = Aucune parole détectée — réessayez.

# Capture d'écran + annotation.
feedback-shot-label = Capture d'écran
feedback-shot-status-capturing = Capture en cours…
feedback-shot-status-attached = Jointe — dessinez dessus pour annoter
feedback-shot-status-none = Aucune capture d'écran
feedback-shot-status-failed = Capture d'écran indisponible
feedback-shot-capture = Ajouter une capture d'écran
feedback-shot-recapture = Recapturer
feedback-shot-remove = Retirer
feedback-shot-exact = Au pixel près
feedback-shot-exact-title = Capture les vrais pixels affichés via le sélecteur de partage d'écran du navigateur — inclut canvas, WebGL et les cadres intégrés que la capture normale ne peut pas reproduire.
feedback-shot-exact-cancelled = Partage d'écran annulé — la capture actuelle est conservée.
feedback-shot-capture-failed = Impossible de réaliser une capture d'écran.

feedback-shot-annotate = Annoter
feedback-annotate-heading = Annoter la capture d'écran
feedback-annotate-done = Retour au formulaire
feedback-annotate-hint = Faites glisser pour dessiner. Utilisez l'outil de déplacement (ou le bouton central de la souris) pour vous déplacer dans une vue zoomée ; ctrl/⌘ + molette pour zoomer.
feedback-tool-pan-title = Déplacer
feedback-zoom-preset-title = Cliquez pour ajuster toute la capture, cliquez à nouveau pour occuper toute la largeur
feedback-zoom-fit-label = Ajuster
feedback-zoom-width-label = Largeur

feedback-tool-rect-title = Rectangle
feedback-tool-arrow-title = Flèche
feedback-tool-pen-title = Main levée
feedback-tool-text-title = Texte
feedback-tool-redact-title = Masquer / caviarder (bloc plein)
feedback-tool-text-prompt = Texte de l'annotation
feedback-color-aria = Couleur
feedback-undo-title = Annuler
feedback-redo-title = Rétablir
feedback-clear-annot-title = Effacer les annotations
feedback-clear-annot-label = Effacer
feedback-zoom-out-title = Dézoomer
feedback-zoom-in-title = Zoomer

# Images supplémentaires, collées ou déposées sur le formulaire.
feedback-attachments-label = Images
feedback-attachments-count = { $count } sur { $max }
feedback-attachments-hint = Collez ou déposez des images ici pour les joindre.
feedback-attachments-drop = Déposez pour joindre
feedback-attachments-remove = Retirer l'image
feedback-attachments-too-many = { $max } images au maximum.
feedback-attachments-too-large = Cette image dépasse { $max } Mo.
feedback-attachments-invalid = Seuls des fichiers image peuvent être joints.

# Consentement aux diagnostics. Les deux sont actifs par défaut ; le journal
# de conversation n'apparaît que sur une page de conversation.
feedback-log-browser-label = Envoyer le journal d'activité du navigateur (console + réseau)
feedback-log-chat-label = Envoyer le journal de conversation et d'outils
feedback-diagnostics-label = Afficher les données qui seront jointes

feedback-confirm-heading = Confirmer ?
feedback-confirm-public-p1-prefix = Ce retour ouvre un ticket dans notre suivi
feedback-confirm-public-p1-strong = public
feedback-confirm-public-p1-suffix = . N'importe qui peut le lire.
feedback-confirm-private-p2-prefix = Assurez-vous que votre capture et les données envoyées ne contiennent
feedback-confirm-private-p2-strong = aucune information personnelle ou privée
feedback-confirm-private-p2-suffix = (noms, e-mails, jetons, données clients, …).
feedback-confirm-cancel-button = Non, je veux modifier
feedback-confirm-ok-button = Oui, envoyer

feedback-thanks-heading = Merci
feedback-thanks-body = Votre retour a été enregistré comme ticket.
feedback-thanks-issue = Enregistré comme ticket #{ $number }.
feedback-thanks-open = Ouvrir le ticket
feedback-done-button = Terminé

feedback-err-no-session = Aucune session active
feedback-err-session-lookup-failed = Échec de la recherche de session
feedback-err-body-read = { $error }
feedback-err-empty-transcript = Transcription vide
feedback-err-malformed-json = JSON invalide : { $error }
feedback-err-no-chat-model = Aucun modèle de chat disponible pour l'extraction
feedback-err-extraction-failed = Échec de l'extraction : { $error }
feedback-err-not-configured = Le retour d'expérience n'est pas configuré
feedback-err-title-required = Le titre est obligatoire (au moins 4 caractères)
feedback-err-description-required = La description est obligatoire
feedback-err-submit-failed = Impossible de créer le ticket — veuillez réessayer
feedback-err-network = Erreur réseau : { $error }
