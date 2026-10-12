# STATUS: llm-generated, unreviewed — pending native-speaker QA

agent-output-withheld = 我无法提供该回答，因为其中提到了我无法为您核实的信息。请换一种方式提问，或联系支持人员。
agent-output-redacted = [已移除]
dot-error-name-taken = 你的账户中已有同名个人 Dot。请更换名称。
dot-error-name-invalid = 请输入不超过 80 个字符且不含控制字符的名称。
dot-error-active-limit = 已达到个人 Dot 启用数量上限。请先停用一个 Dot，再创建新的 Dot。
dot-error-not-found = 你无法访问此 Dot。
dot-error-id-missing = 请求中缺少 Dot ID。
dot-memory-error-kind = 请选择偏好、项目或事实作为记忆类型。
dot-memory-error-empty = 记忆内容不能为空。
dot-memory-error-id-missing = 请求中缺少 Dot ID 或记忆 ID。

# What a website visitor reads next to the secure field when an identity
# verifier (#95) asks for the code it sent. Worded the same whether or not
# the address is registered, so it reveals neither.
agent-verifier-code-sent = 如果此地址已在我们这里注册，我们已向其发送了验证码。请在此输入验证码。
agent-verifier-code-again = 请输入我们发送给您的验证码。

# What a website visitor reads when the public agent endpoint refuses a
# request (#92): the agent's budget is spent, or a rate limit was hit.
agent-embed-unavailable = 此助手暂时不可用。请稍后再试，或使用本网站的其他联系方式。
agent-embed-rate-limited = 您发送消息的速度超过了此助手的接收速度。请等待 { $seconds } 秒后再试。

agent-embed-decision-for-staff = 此请求正在等待我们的工作人员处理。一旦做出决定，您将在此处看到答复。
agent-embed-not-waiting = 助手目前没有在等待您的回答。请重新加载对话。
agent-embed-message-waiting = 您的上一条消息仍在等待助手处理。请等待其答复后再发送新消息。

agent-a2a-input-required = 合作服务还需要一项信息才能继续。请在此输入；该信息只会发送给该服务。
dot-error-inactive = 此 Dot 已停用。请先启用它，再开始对话。
dot-action-chat = 聊天
dot-action-chat-opening = 正在打开聊天…
dot-activity-session-started = 已开始与 Dot 聊天。

dot-workspace-error-unavailable = Dot 的持久存储不可用。请管理员检查沙箱服务。
dot-workspace-error-conflict = 工作区正忙或已更改。请刷新，并在 Dot 空闲时重试。
dot-workspace-error-not-found = 未找到该条目。请刷新目录。
dot-workspace-error-too-large = 文件或目录超过工作区限制。请使用更小的文件或目录。
dot-workspace-error-invalid = 请求无效。请检查路径后重试。
dot-workspace-error-failed = 操作失败。请刷新后重试。

dot-workspace-heading = 工作区
dot-workspace-intro = 此工作区中的文件会在 Dot 的多次运行之间保留。您可以下载文件进行查看。
dot-workspace-reload = 重新加载工作区
dot-workspace-unavailable = 持久存储不可用。请联系管理员检查沙箱服务。
dot-workspace-busy = 此 Dot 正在使用工作区。文件可能发生变化。Dot 空闲时，您可以上传文件、创建文件夹或删除内容。
dot-workspace-empty = 此文件夹为空。
dot-workspace-root = 工作区根目录
dot-workspace-folder = 文件夹
dot-workspace-size-missing = 文件大小不可用
dot-workspace-modified = 修改时间：{ $when }
dot-workspace-upload = 上传文件
dot-workspace-file = 文件
dot-workspace-upload-limit = 请选择不超过 50 MiB 的文件。
dot-workspace-create-folder = 创建文件夹
dot-workspace-folder-name = 文件夹名称
dot-workspace-delete = 删除
dot-workspace-delete-confirm = 删除“{ $name }”？文件夹必须为空。
dot-workspace-more = 加载更多文件
dot-workspace-error-load = 无法加载工作区。请重新加载后重试。
dot-workspace-error-save = 无法保存更改。请重新加载工作区后重试。
dot-workspace-notice-saved = 工作区已更新。
