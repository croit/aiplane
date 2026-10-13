# STATUS: llm-generated, unreviewed — pending native-speaker QA

tools-heading = 工具
tools-description = 打开或关闭助手可以使用的工具。更改仅适用于您的账户，并在您发送下一条消息时生效。
tools-none-granted = 您的角色未授予任何工具。

tools-location-heading = 位置
tools-location-description = 分享您设备的精确位置，以便助手回答诸如“这里天气怎么样？”之类的问题。它仅用于您的工具调用，您可以随时停止共享。如果不共享，助手将退回使用根据您的 IP 地址推断的大致位置。
tools-location-share-button = 分享精确位置
tools-location-stop-button = 停止共享
tools-location-shared = 已共享。
tools-location-shared-accuracy = 已共享 — 精度 ±{ $accuracy } 米。
tools-location-not-shared = 未共享。
tools-location-unavailable = 无法访问您的位置。请检查浏览器的位置权限后重试。

# SPA-only: the Svelte /tools toggle list.
tools-toggle-aria = 切换 { $name }
tools-no-description = 无描述

# Section headings for the tool catalog, shared by /tools, the per-token
# capability panel, the chat capability picker and the admin grant matrix.
tool-category-web-network = 网络
tool-category-attachments-documents = 附件与文档
tool-category-document-templates = 文档模板
tool-category-knowledge-base = 知识库
tool-category-code-sandbox = 代码与沙盒
tool-category-memory = 记忆
tool-category-integrations = 集成
tool-category-utility = 实用工具
tool-category-skills = 技能
tool-category-images-media = 图像与媒体
tool-category-comfyui-workflows = ComfyUI 工作流
tool-category-scheduled-actions = 计划操作

tools-configure-link = 配置
tools-needs-storage = 需要文件存储。
tools-needs-rag = 需要知识库。
tools-needs-push = 需要推送通知。
tools-needs-geoip = 需要 GeoIP 数据库。
tools-needs-image-backend = 需要图像后端。
tools-needs-sandbox = 需要沙盒运行服务。
tools-needs-sandbox-network = 需要沙盒网络访问权限。

# SPA-only: /tools/browser, setting up the Chrome extension browser_control drives.
tools-browser-tab = 浏览器扩展
tools-browser-description = 让对话在此浏览器中以你的登录身份操作只有你能访问的页面：内部工具、单点登录后的页面、只有你能提交的表单。仅在对话保持打开且你已开启扩展时有效。
tools-browser-status-heading = 状态
tools-browser-status-not-granted = 你的角色未授予浏览器控制权限。如有需要，请联系管理员。
tools-browser-status-tool-off = 浏览器控制已在你的工具列表中关闭。
tools-browser-status-not-detected = 此页面上没有扩展响应。请安装扩展，并在其设置中添加此 AIplane。
tools-browser-status-switched-off = 扩展已安装并配对，但处于关闭状态。
tools-browser-status-ready = 已就绪。助手可以在此浏览器中操作。
tools-browser-switch-on = 开启
tools-browser-switch-on-fallback = Chrome 没有打开扩展窗口。请点击工具栏中的扩展图标，然后选择“开启”。
tools-browser-open-tools = 打开工具列表
tools-browser-recheck = 重新检查
tools-browser-install-heading = 安装
tools-browser-store-button = Chrome 网上应用店
tools-browser-download-button = 下载 .zip
tools-browser-download-note = .zip 用于以解压方式安装：适用于 Linux 或 Chrome 开发者模式。在 Windows 和 macOS 上，Chrome 只能从应用店安装扩展。
tools-browser-steps-heading = 设置
tools-browser-step-install = 从 Chrome 网上应用店安装扩展。需要 Chrome 127 或更高版本。
tools-browser-step-pair = 打开扩展设置并添加此 AIplane 的地址。Chrome 随后会请求该地址的权限；允许后即完成配对。
tools-browser-step-switch-on = 回到此页面并点击“开启”，或使用工具栏中的扩展图标。首次使用时，Chrome 会请求访问网站的权限。
tools-browser-step-ask = 在对话中提出请求，例如：“打开我们的内部 Wiki 并总结入职页面。”
tools-browser-unpacked-heading = 以解压方式安装
tools-browser-unpacked-steps = 解压下载的文件，打开 chrome://extensions，开启开发者模式，点击“加载已解压的扩展程序”，然后选择解压后的文件夹。
tools-browser-origin-label = 此 AIplane 的地址
tools-browser-copy-origin = 复制
tools-browser-copied = 已复制
tools-browser-notes-heading = 须知
tools-browser-note-window = 助手在自己的窗口中工作，位于名为“Assistant”的标签组内。扩展开启期间，其图标为绿色，Chrome 会显示浏览器正在被调试的提示栏。
tools-browser-note-open = 仅在对话于标签页中保持打开时有效。完成后请通过扩展图标将其关闭。
tools-browser-note-injection = 助手读取的页面可能包含针对它的文字。你可以在扩展设置中将其限制为你批准的网站。

tool-forward-request-title = 询问另一个 Dot
tool-forward-request-description = 向另一个个人 Dot 发送明确的任务。两个 Dot 都必须允许该请求；接收方 Dot 使用自己的工具和批准规则。
