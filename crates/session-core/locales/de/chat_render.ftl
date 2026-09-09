# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/render.rs` — the
# gateway-only chat-page chrome: the header model/voice pickers, the
# compliance banners, the composer's "+" tools/integrations/skills menu,
# the "Denken" (effort/thinking) picker, and the share/export/fork
# controls. Prefixed `chat-render-` (rather than `chat-`) to avoid
# colliding with `chat/mod.rs`'s own `chat-*` keys in the sibling
# `chat.ftl`.

chat-render-model-placeholder = Modell (z. B. gpt-4o-mini)
chat-render-model-aria = Chat-Modell

chat-render-composer-placeholder = Nachricht an das Modell…

chat-render-effort-title = Denkaufwand
chat-render-effort-tooltip = Denkaufwand: höher = mehr Reasoning und mehr Tool-Runden, aber langsamer
chat-render-effort-label-prefix = Denkaufwand:
chat-render-effort-fast = Schnell
chat-render-effort-standard = Standard
chat-render-effort-deep = Tief
chat-render-effort-max = Maximal

chat-render-tools-tooltip = Tools, Integrationen & Skills für diese Unterhaltung
chat-render-tools-label = Tools

chat-render-close = Schließen

chat-render-share-label-on = Freigegeben ✓
chat-render-share-label-off = Freigeben
chat-render-share-tooltip = Freigegebene Chats können von jedem angemeldeten Benutzer mit dem Link gelesen werden

chat-render-fork-tooltip = Diese Unterhaltung in deine eigenen Chats kopieren, um weiterzuchatten
chat-render-fork-label = In meinen Chats fortsetzen

chat-render-export-tooltip = Diese Unterhaltung herunterladen
chat-render-export-aria = Unterhaltung exportieren
chat-render-export-label = Exportieren
chat-render-export-pdf = PDF-Dokument
chat-render-export-md = Markdown (.md)

# SPA-only chat-page chrome (`web/src/routes/chat/[id]`): the canvas
# document list and its save state, the composer's per-tool remove
# button, the model picker's compliance tooltips, and the attachment
# size chip.
chat-render-documents-label = Dokumente
chat-render-revision-count = { $count ->
    [one] { $count } Version
   *[other] { $count } Versionen
}
chat-render-canvas-saving = Speichern …
chat-render-tool-disable-aria = { $name } deaktivieren
chat-render-model-gdpr-region = DSGVO-Region
chat-render-model-nda-covered = Durch NDA abgedeckt
chat-render-attachment-size-kb = { $size } KB
