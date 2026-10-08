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
chat-error-attachment-storage-failed = Le stockage des pièces jointes n'a pas répondu correctement, le fichier n'a donc pas pu être récupéré. Réessayez ; si l'échec persiste, un administrateur doit vérifier le stockage des pièces jointes dans /admin/settings.
chat-error-turn-interrupted = Une erreur interne a interrompu cette réponse. Veuillez réessayer.

# SPA-only chat chrome (`web/src/routes/chat/*`): the conversation list,
# the conversation header, the assistant's ask-back card, and the
# turn-status line the server never renders itself.
chat-list-empty = Aucune conversation pour l'instant. Lancez-en une ci-dessus.
chat-turn-stopped = arrêté
chat-waiting-approval = N’exécute { $tool } qu’une fois que vous l’avez approuvé.
chat-waiting-value = Attend une valeur que vous seul pouvez saisir. Elle va à l’outil, jamais au modèle.
chat-waiting-answer = Attend la réponse d’une personne.
chat-composer-paused = Approuvez ou refusez d’abord la demande ci-dessus ; le message suivant attend cette décision.
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

linked-chat-label = Continuer dans
linked-chat-fresh = Un nouveau chat, ouvert par la prochaine exécution
linked-chat-help = Chaque exécution ajoute son prompt et sa réponse à ce chat. Choisissez une de vos conversations pour tout garder dans un seul fil.

# A model call that looped and was retried at a lower thinking level, shown
# collapsed above the answer. $effort is a level name (off/low/medium/high/xhigh).
chat-attempt-summary = Tentative { $n } arrêtée : { $reason } (réflexion : { $effort })
chat-attempt-reason-loop = le modèle s’est répété
chat-attempt-reason-repeated_call = le même appel d’outil, encore et encore
chat-attempt-retry = Nouvelle tentative avec la réflexion { $effort } (tentative { $n })
chat-loop-exhausted = Le modèle s’est répété lors des { $attempts } tentatives, même avec moins de réflexion. Reformulez la question ou découpez-la en parties plus petites.
