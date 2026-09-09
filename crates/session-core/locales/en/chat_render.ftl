# Strings owned by `gateway/src/rama_server/pages/chat/render.rs` — the
# gateway-only chat-page chrome: the header model/voice pickers, the
# compliance banners, the composer's "+" tools/integrations/skills menu,
# the "Denken" (effort/thinking) picker, and the share/export/fork
# controls. Prefixed `chat-render-` (rather than `chat-`) to avoid
# colliding with `chat/mod.rs`'s own `chat-*` keys in the sibling
# `chat.ftl`.

chat-render-model-placeholder = model (e.g. gpt-4o-mini)
chat-render-model-aria = Chat model

chat-render-composer-placeholder = Message the model…

chat-render-effort-title = Thinking effort
chat-render-effort-tooltip = Thinking effort: higher = more reasoning and more tool rounds, but slower
chat-render-effort-label-prefix = Thinking:
chat-render-effort-fast = Fast
chat-render-effort-standard = Standard
chat-render-effort-deep = Deep
chat-render-effort-max = Max

chat-render-tools-tooltip = Tools, integrations & skills for this conversation
chat-render-tools-label = Tools

chat-render-close = Close

chat-render-share-label-on = Shared ✓
chat-render-share-label-off = Share
chat-render-share-tooltip = Shared chats are readable by any signed-in user who has the link

chat-render-fork-tooltip = Copy this conversation into your own chats so you can keep chatting
chat-render-fork-label = Continue in my chats

chat-render-export-tooltip = Download this conversation
chat-render-export-aria = Export conversation
chat-render-export-label = Export
chat-render-export-pdf = PDF document
chat-render-export-md = Markdown (.md)

# SPA-only chat-page chrome (`web/src/routes/chat/[id]`): the canvas
# document list and its save state, the composer's per-tool remove
# button, the model picker's compliance tooltips, and the attachment
# size chip.
chat-render-documents-label = Documents
chat-render-revision-count = { $count ->
    [one] { $count } revision
   *[other] { $count } revisions
}
chat-render-canvas-saving = Saving…
chat-render-tool-disable-aria = Disable { $name }
chat-render-model-gdpr-region = GDPR region
chat-render-model-nda-covered = NDA-covered
chat-render-attachment-size-kb = { $size } KB
