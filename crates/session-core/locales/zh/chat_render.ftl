# STATUS: llm-generated, unreviewed — pending native-speaker QA
# Strings owned by `gateway/src/rama_server/pages/chat/render.rs` — the
# gateway-only chat-page chrome: the header model/voice pickers, the
# compliance banners, the composer's "+" tools/integrations/skills menu,
# the "Denken" (effort/thinking) picker, and the share/export/fork
# controls. Prefixed `chat-render-` (rather than `chat-`) to avoid
# colliding with `chat/mod.rs`'s own `chat-*` keys in the sibling
# `chat.ftl`.

chat-render-model-placeholder = 模型（例如 gpt-4o-mini）
chat-render-model-aria = 聊天模型

chat-render-composer-placeholder = 给模型发消息…

chat-render-effort-title = 思考强度
chat-render-effort-tooltip = 思考强度：越高 = 推理越多、工具调用轮次越多，但速度更慢
chat-render-effort-label-prefix = 思考：
chat-render-effort-fast = 快速
chat-render-effort-standard = 标准
chat-render-effort-deep = 深度
chat-render-effort-max = 最大

chat-render-tools-tooltip = 本次对话的工具、集成与技能
chat-render-tools-label = 工具

chat-render-close = 关闭

chat-render-share-label-on = 已共享 ✓
chat-render-share-label-off = 共享
chat-render-share-tooltip = 已共享的对话，任何持有链接的已登录用户都可以阅读

chat-render-fork-tooltip = 将此对话复制到您自己的聊天中，以便继续对话
chat-render-fork-label = 在我的聊天中继续

chat-render-export-tooltip = 下载此对话
chat-render-export-aria = 导出对话
chat-render-export-label = 导出
chat-render-export-pdf = PDF 文档
chat-render-export-md = Markdown (.md)

# SPA-only chat-page chrome (`web/src/routes/chat/[id]`): the canvas
# document list and its save state, the composer's per-tool remove
# button, the model picker's compliance tooltips, and the attachment
# size chip.
chat-render-documents-label = 文档
chat-render-revision-count = { $count } 个版本
chat-render-canvas-saving = 正在保存…
chat-render-tool-disable-aria = 停用 { $name }
chat-render-model-gdpr-region = GDPR 区域
chat-render-model-nda-covered = 受 NDA 保护
chat-render-attachment-size-kb = { $size } KB
