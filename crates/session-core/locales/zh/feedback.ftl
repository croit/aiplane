# 反馈小部件：悬浮按钮、对话框（表单、语音输入、截图标注、附件、诊断数据授权）、
# 公开跟踪器确认，以及 `/api/v0/feedback*` 的错误消息。

feedback-fab-aria = 发送反馈
feedback-fab-title = 发送反馈

feedback-dialog-heading = 发送反馈
feedback-dialog-blurb = 当前页面、你的浏览器以及最近的控制台与网络活动会自动附上。
feedback-close-aria = 关闭
feedback-cancel-button = 取消
feedback-submit-button = 发送反馈
feedback-sending = 发送中…

feedback-title-label = 标题
feedback-title-placeholder = 简短摘要
feedback-description-label = 描述
feedback-description-placeholder = 发生了什么，或者你希望有什么？
feedback-business-label = 业务价值
feedback-business-placeholder = 为什么这很重要？谁会受到影响？
feedback-acceptance-label = 验收标准
feedback-acceptance-placeholder = 什么情况算完成？
feedback-priority-label = 优先级
feedback-priority-low = 低
feedback-priority-medium = 中
feedback-priority-high = 高

# 语音输入——一次录音即可填好上面所有字段。
feedback-voice-button-label = 用语音填写
feedback-voice-button-title = 点一下，描述问题，再点一下——我们会帮你填好下面的字段
feedback-voice-stop-label = 停止并填写
feedback-voice-working-label = 转写中…
feedback-voice-applied = 已根据你的录音填写——请检查后发送。
feedback-voice-no-speech = 未检测到语音——请重试。

# 截图与标注。
feedback-shot-label = 截图
feedback-shot-status-capturing = 截取中…
feedback-shot-status-attached = 已附上——在上面绘制即可标注
feedback-shot-status-none = 没有截图
feedback-shot-status-failed = 无法获取截图
feedback-shot-capture = 添加截图
feedback-shot-recapture = 重新截取
feedback-shot-remove = 移除
feedback-shot-exact = 像素级精确
feedback-shot-exact-title = 通过浏览器的屏幕共享选择器截取真实的屏幕像素——包含普通截图无法还原的 canvas、WebGL 和内嵌框架。
feedback-shot-exact-cancelled = 已取消屏幕共享——保留当前截图。
feedback-shot-capture-failed = 无法截取屏幕。

feedback-shot-annotate = 标注
feedback-annotate-heading = 标注截图
feedback-annotate-done = 返回表单
feedback-annotate-hint = 拖动即可绘制。用移动工具（或按住鼠标中键）平移放大后的画面；ctrl/⌘ + 滚轮缩放。
feedback-tool-pan-title = 移动 / 平移
feedback-zoom-preset-title = 点击缩放到整张截图，再次点击则铺满宽度
feedback-zoom-fit-label = 适应
feedback-zoom-width-label = 宽度

feedback-tool-rect-title = 矩形
feedback-tool-arrow-title = 箭头
feedback-tool-pen-title = 自由绘制
feedback-tool-text-title = 文字
feedback-tool-redact-title = 遮盖 / 涂黑（实心块）
feedback-tool-text-prompt = 标注文字
feedback-color-aria = 颜色
feedback-undo-title = 撤销
feedback-redo-title = 重做
feedback-clear-annot-title = 清除标注
feedback-clear-annot-label = 清除
feedback-zoom-out-title = 缩小
feedback-zoom-in-title = 放大

# 额外图片，可粘贴或拖放到表单上。
feedback-attachments-label = 图片
feedback-attachments-count = { $count } / { $max }
feedback-attachments-hint = 把图片粘贴或拖放到这里即可附上。
feedback-attachments-drop = 松开以附上
feedback-attachments-remove = 移除图片
feedback-attachments-too-many = 最多 { $max } 张图片。
feedback-attachments-too-large = 该图片超过 { $max } MB。
feedback-attachments-invalid = 只能附上图片文件。

# 诊断数据授权。两项默认开启；聊天日志仅在对话页面出现。
feedback-log-browser-label = 提交浏览器活动日志（控制台 + 网络）
feedback-log-chat-label = 提交聊天与工具使用日志
feedback-diagnostics-label = 查看将要附上的数据

feedback-confirm-heading = 确定吗？
feedback-confirm-public-p1-prefix = 这条反馈会在我们的
feedback-confirm-public-p1-strong = 公开
feedback-confirm-public-p1-suffix = issue 跟踪器中创建工单，任何人都能看到。
feedback-confirm-private-p2-prefix = 请确认你的截图和提交的数据中
feedback-confirm-private-p2-strong = 不含任何个人或私密信息
feedback-confirm-private-p2-suffix = （姓名、邮箱、令牌、客户数据……）。
feedback-confirm-cancel-button = 先不发，我再改改
feedback-confirm-ok-button = 确认发送

feedback-thanks-heading = 谢谢
feedback-thanks-body = 你的反馈已创建为 issue。
feedback-thanks-issue = 已创建 issue #{ $number }。
feedback-thanks-open = 打开该 issue
feedback-done-button = 完成

feedback-err-no-session = 没有活动会话
feedback-err-session-lookup-failed = 查询会话失败
feedback-err-body-read = { $error }
feedback-err-empty-transcript = 转写内容为空
feedback-err-malformed-json = JSON 格式错误：{ $error }
feedback-err-no-chat-model = 没有可用于抽取的聊天模型
feedback-err-extraction-failed = 抽取失败：{ $error }
feedback-err-not-configured = 尚未配置反馈功能
feedback-err-title-required = 必须填写标题（至少 4 个字符）
feedback-err-description-required = 必须填写描述
feedback-err-submit-failed = 无法创建 issue——请重试
feedback-err-network = 网络错误：{ $error }
