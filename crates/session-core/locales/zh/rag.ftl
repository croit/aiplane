# STATUS: llm-generated, unreviewed — pending native-speaker QA

rag-heading = RAG 集合

# Toasts — collection CRUD
rag-toast-vanished = 保存后集合消失了。

# Toasts — refs / sources
rag-toast-bulk-queued-skipped = 已排队 { $added } 个源；已跳过 { $skipped } 个重复项。
rag-toast-bulk-queued = 已排队 { $added } 个源的索引任务。
rag-toast-reindex-queued-ref = 已安排 `{ $ref }` 的重新索引。

# Status badges
rag-status-pending = 待处理
rag-status-cloning = 克隆中
rag-status-indexing = 索引中
rag-status-ready = 就绪
rag-status-error = 错误

# Collection row
rag-button-edit = 编辑
rag-button-add-source = 添加源
rag-button-add-bulk = 批量添加源

# Ref / source row
rag-badge-primary = 主要
rag-button-reindex = 重新索引
rag-button-set-primary = 设为主要
rag-button-remove = 移除

# Inline per-source editor
rag-label-branch-tag = 分支 / 标签
rag-button-cancel = 取消

# Create-collection form
rag-label-name = 名称
rag-label-chunk-size = 分块大小
rag-label-chunk-overlap = 分块重叠

# Edit-collection form
rag-label-description = 描述

# Embedding model field
rag-label-embedding-model = Embedding 模型

# 来源选择器和提供方凭据（rag_source.rs）。各字段标签由提供方自身给出，
# 不做翻译。
rag-label-source-kind = 来源
rag-source-unknown-kind = 未知的来源类型。
rag-source-test-button = 测试连接
rag-source-test-ok = 已以 `{ $account }` 连接。所配置文件夹下有 { $entries } 个项目。
rag-source-test-ok-plain = 已连接。所配置文件夹下有 { $entries } 个项目。
rag-source-test-failed = 无法访问来源：{ $error }
rag-source-detected = 已检测到：{ $server }

rag-label-profile = 文档字段
rag-option-profile-none = 无 — 仅索引文本

# 同步钩子 —— 触发单个集合重新同步的入站请求。
rag-button-sync-token = 同步 URL
rag-badge-sync-hook = 同步钩子

# Browser consent for an OAuth source (Google Drive).
rag-source-consent-save-first = 请先保存包含客户端 ID 和密钥的集合，然后再连接以授予访问权限。
rag-oauth-lookup-failed = 无法读取该集合。
rag-oauth-not-oauth = 此来源类型不通过浏览器连接。
rag-oauth-no-client = 请先在集合中保存 OAuth 客户端 ID 和密钥。
rag-oauth-bad-authorize-url = 无法构建提供方的授权 URL。
rag-oauth-start-failed = 无法开始授权。
rag-oauth-callback-missing = 提供方的响应缺少 code 或 state。
rag-oauth-expired = 该授权已过期或已被使用，请重新开始。
rag-oauth-provider-refused = 提供方拒绝了授权：{ $error }
rag-oauth-exchange-failed = 交换授权码失败：{ $error }
rag-oauth-no-refresh-token = 提供方未返回刷新令牌，网关将无法在无人值守时继续索引。请在提供方账号中撤销网关的访问权限后重新连接。
rag-oauth-store-failed = 无法保存凭据。

# SPA 集合管理：创建表单、同步 URL 卡片、源列表行，以及旧页面不需要的确认提示。
rag-button-new-collection = 新建集合
rag-label-git-url = Git URL
rag-source-testing = 测试中…
rag-button-create = 创建
rag-button-rebuild = 重建
rag-sync-url-heading = 同步 URL——仅显示一次
rag-sync-token-confirm = 要生成新的同步 URL 吗？旧的将失效。
rag-delete-collection-confirm = 要删除集合 { $name } 及其索引吗？
rag-remove-source-confirm = 要移除源 { $source } 吗？
rag-toast-rebuild-queued = 已请求完整重建。
rag-ref-indexed-at = 已于 { $date } 建立索引
rag-no-sources = 暂无源——在添加源之前，此集合不会索引任何内容。
rag-add-sources-hint = 每行一个源；添加 { $at } 可覆盖此集合的 { $ref }。
