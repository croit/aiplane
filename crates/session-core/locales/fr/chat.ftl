# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/mod.rs` — the
# multi-conversation chat page's server-side handlers: page title
# fallback, sidebar/effort/share/pin toasts, and the SSE-toast error
# messages the composer's fetch layer surfaces on failed actions.

chat-default-title = Chat

chat-error-still-streaming = Une réponse est toujours en cours pour cet utilisateur — attendez ou appuyez sur Arrêter.

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
chat-list-pinned-badge = épinglée
chat-list-pin = Épingler
chat-list-unpin = Désépingler
chat-list-delete = Supprimer
chat-push-invite = Soyez averti dès qu'une réponse est terminée.
chat-all-chats = Toutes les discussions
chat-turn-stopped = arrêté
chat-prompt-heading = L'assistant demande
chat-prompt-placeholder = Saisissez une réponse…
chat-prompt-answer = Répondre
chat-prompt-skip = Ignorer
