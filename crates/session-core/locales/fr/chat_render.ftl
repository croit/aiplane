# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/render.rs` — the
# gateway-only chat-page chrome: the header model/voice pickers, the
# compliance banners, the composer's "+" tools/integrations/skills menu,
# the "Denken" (effort/thinking) picker, and the share/export/fork
# controls. Prefixed `chat-render-` (rather than `chat-`) to avoid
# colliding with `chat/mod.rs`'s own `chat-*` keys in the sibling
# `chat.ftl`.

chat-render-model-placeholder = modèle (ex. gpt-4o-mini)
chat-render-model-aria = Modèle de chat

chat-render-composer-placeholder = Écrire au modèle…

chat-render-effort-title = Effort de réflexion
chat-render-effort-tooltip = Effort de réflexion : plus élevé = plus de raisonnement et de cycles d'outils, mais plus lent
chat-render-effort-label-prefix = Réflexion :
chat-render-effort-fast = Rapide
chat-render-effort-standard = Standard
chat-render-effort-deep = Approfondi
chat-render-effort-max = Maximal

chat-render-tools-tooltip = Outils, intégrations et skills pour cette conversation
chat-render-tools-label = Outils

chat-render-close = Fermer

chat-render-share-label-on = Partagé ✓
chat-render-share-label-off = Partager
chat-render-share-tooltip = Les chats partagés sont lisibles par tout utilisateur connecté disposant du lien

chat-render-fork-tooltip = Copier cette conversation dans vos propres chats pour continuer à discuter
chat-render-fork-label = Continuer dans mes chats

chat-render-export-tooltip = Télécharger cette conversation
chat-render-export-aria = Exporter la conversation
chat-render-export-label = Exporter
chat-render-export-pdf = Document PDF
chat-render-export-md = Markdown (.md)

# SPA-only chat-page chrome (`web/src/routes/chat/[id]`): the canvas
# document list and its save state, the composer's per-tool remove
# button, the model picker's compliance tooltips, and the attachment
# size chip.
chat-render-documents-label = Documents
chat-render-revision-count = { $count ->
    [one] { $count } révision
   *[other] { $count } révisions
}
chat-render-canvas-saving = Enregistrement…
chat-render-tool-disable-aria = Désactiver { $name }
chat-render-model-gdpr-region = Région RGPD
chat-render-model-nda-covered = Couvert par un NDA
chat-render-attachment-size-kb = { $size } Ko
