# Strings owned by `gateway/src/rama_server/pages/mod.rs` — the app
# sidebar, authed page shell, login page, and the generic
# error/forbidden pages. Lives in session-core's locales/ alongside
# chrome.ftl per the shared-translation-source architecture (both
# binaries render the same chrome).

nav-memory = Memory
nav-scheduled = Scheduled
nav-webhooks = Webhooks
nav-integrations = Integrations
nav-tools = Tools
nav-tokens = Tokens
nav-usage = Usage
nav-users = Users
nav-admin-tokens = API tokens
nav-groups = Groups
nav-models = Models
nav-upstreams = Upstreams
nav-rag = RAG
nav-skills = Skills
nav-connectors = Connectors
nav-limits = Limits

nav-group-workspace = Workspace
nav-group-account = Account
nav-group-admin = Admin
nav-group-toggle-aria = Toggle { $label } section

nav-conversations-label = Conversations
nav-new-conversation-aria = Start a new conversation
nav-new-conversation-title = New conversation
nav-untitled-chat = Untitled chat
nav-pin-conversation = Pin conversation
nav-unpin-conversation = Unpin conversation
nav-delete-conversation = Delete conversation
nav-search-placeholder = Search…
nav-search-aria = Search

nav-sign-out = Sign out

nav-my-skills = My Skills
nav-settings = Settings

# The SPA shell's own chrome: the mobile drawer and the theme toggle,
# which the server-rendered shell had no equivalent of.
nav-comfyui = ComfyUI
nav-close-menu = Close menu
nav-open-menu = Open menu
nav-main-aria = Main navigation
nav-search-close-aria = Close search

# A failed sign-out. Silence here would show a signed-out shell over a
# live session, which on a shared machine is the worst outcome.
nav-sign-out-failed = Signing out failed, so you are still signed in: { $error }
