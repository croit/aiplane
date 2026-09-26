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
chat-error-turn-interrupted = Une erreur interne a interrompu cette réponse. Veuillez réessayer.

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
chat-composer-send-during-turn-title = Envoyer. Pendant qu'une réponse s'écrit, ceci lui est ajouté ; si cela arrive trop tard, c'est envoyé comme message suivant.
chat-composer-interrupt = Interrompre et réorienter
chat-composer-interrupt-title = Arrêter la réponse en cours et envoyer ceci à la place. Ce qui a déjà été écrit reste dans la conversation.
chat-turn-waiting = Envoyé — en attente d'un créneau libre
chat-turn-waiting-cancel = Reprendre
chat-steer-pending = Ajouté pendant cette réponse — pas encore lu
chat-steer-delivered = Ajouté pendant cette réponse — pris en compte
chat-steer-resent = Ajouté pendant cette réponse — arrivé trop tard, envoyé comme message suivant
chat-steer-discarded = Ajouté pendant cette réponse — arrivé trop tard et abandonné
