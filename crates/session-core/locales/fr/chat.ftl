# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/mod.rs` — the
# multi-conversation chat page's server-side handlers: page title
# fallback, sidebar/effort/share/pin toasts, and the SSE-toast error
# messages the composer's fetch layer surfaces on failed actions.

chat-default-title = Chat

chat-error-auth-required = authentification requise
chat-error-no-such-turn = ce message n'existe pas
chat-error-db-error = erreur de base de données
chat-error-attachments-not-configured = les pièces jointes du chat ne sont pas configurées
chat-error-bad-filename = nom de fichier invalide
chat-error-attachment-not-found = introuvable

# SPA-only chat chrome (`web/src/routes/chat/*`): the conversation list,
# the conversation header, the assistant's ask-back card, and the
# turn-status line the server never renders itself.
chat-list-empty = Aucune conversation pour l'instant. Lancez-en une ci-dessus.
chat-turn-stopped = arrêté
chat-prompt-heading = L'assistant demande
chat-prompt-placeholder = Saisissez une réponse…
chat-prompt-answer = Répondre
chat-prompt-skip = Ignorer

# La zone de saisie reste utilisable pendant qu'une réponse s'écrit.
chat-queue-label = { $count ->
    [one] { $count } message en attente
   *[other] { $count } messages en attente
  }
chat-queue-move-up = Monter
chat-queue-move-down = Descendre
chat-queue-edit = Remettre dans la zone de saisie
chat-queue-remove = Abandonner
chat-queue-held-title = La pièce jointe a été perdue au rechargement — rattachez-la ou abandonnez ce message
chat-queue-held-hint = Un message en attente a perdu sa pièce jointe au rechargement de la page. Il ne partira pas tant que vous ne l'aurez pas remis dans la zone de saisie avec le fichier.
chat-queue-was-interjection = Saisi pendant la réponse précédente, terminée avant de l'avoir lu
chat-composer-interject = Ajouter à la réponse en cours
chat-composer-interject-title = Transmis à la réponse en cours d'écriture (Ctrl/Cmd+Entrée). Arrive à la prochaine étape d'outil ; si la réponse se termine avant, c'est envoyé comme message suivant.
chat-composer-interrupt = Interrompre et réorienter
chat-composer-interrupt-title = Arrêter la réponse en cours et envoyer ceci à la place. Ce qui a déjà été écrit reste dans la conversation.
chat-steer-pending = Ajouté pendant cette réponse — pas encore lu
chat-steer-delivered = Ajouté pendant cette réponse — pris en compte
chat-steer-resent = Ajouté pendant cette réponse — arrivé trop tard, envoyé comme message suivant
chat-steer-discarded = Ajouté pendant cette réponse — arrivé trop tard et abandonné
