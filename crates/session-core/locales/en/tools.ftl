# Strings owned by `gateway/src/rama_server/pages/tools.rs` — the
# per-user tool toggle page and the browser-location sharing card
# shown on it.

tools-heading = Tools
tools-description = Turn the tools the assistant may use on or off. Changes apply to your account only and take effect on your next message.
tools-none-granted = Your roles don't grant any tools.

tools-location-heading = Location
tools-location-description = Share your device's precise location so the assistant can answer questions like "what's the weather here?". It's used only for your tool calls and you can stop sharing anytime. Without it, the assistant falls back to an approximate location derived from your IP address.
tools-location-share-button = Share precise location
tools-location-stop-button = Stop sharing
tools-location-shared = Shared.
tools-location-shared-accuracy = Shared — accuracy ±{ $accuracy } m.
tools-location-not-shared = Not shared.
tools-location-unavailable = Couldn't access your location. Check your browser's location permission and try again.

# SPA-only: the Svelte /tools toggle list.
tools-toggle-aria = Toggle { $name }
# A capability row (chat picker, agent setup) whose resource has no description of its own.
tools-no-description = No description

# Section headings for the tool catalog, shared by /tools, the per-token
# capability panel, the chat capability picker and the admin grant matrix.
tool-category-web-network = Web & Network
tool-category-attachments-documents = Attachments & Documents
tool-category-document-templates = Document templates
tool-category-knowledge-base = Knowledge base
tool-category-code-sandbox = Code & Sandbox
tool-category-memory = Memory
tool-category-integrations = Integrations
tool-category-utility = Utility
tool-category-skills = Skills
tool-category-images-media = Images & Media
tool-category-comfyui-workflows = ComfyUI workflows
tool-category-scheduled-actions = Scheduled actions

tools-configure-link = Configure
tools-needs-storage = Needs file storage.
tools-needs-rag = Needs a knowledge base.
tools-needs-push = Needs push notifications.
tools-needs-geoip = Needs a GeoIP database.
tools-needs-image-backend = Needs an image backend.
tools-needs-sandbox = Needs a sandbox runner.
tools-needs-sandbox-network = Needs sandbox network access.

# SPA-only: /tools/browser, setting up the Chrome extension browser_control drives.
tools-browser-tab = Browser extension
tools-browser-description = Let a conversation act in this browser, with your logins, on pages only you can reach: an internal tool, a page behind single sign-on, a form only you may submit. It works only while the conversation is open, and only after you switch the extension on.
tools-browser-status-heading = Status
tools-browser-status-not-granted = Your roles don't grant browser control. Ask your administrator if you need it.
tools-browser-status-tool-off = Browser control is switched off in your tool list.
tools-browser-status-not-detected = No extension answered on this page. Install it and add this AIplane in its settings.
tools-browser-status-switched-off = The extension is installed and paired, but switched off.
tools-browser-status-ready = Ready. The assistant can act in this browser.
tools-browser-switch-on = Switch on
tools-browser-switch-on-fallback = Chrome didn't open the extension's window. Click the extension's icon in the toolbar and choose Switch on there.
tools-browser-open-tools = Open tool list
tools-browser-recheck = Check again
tools-browser-install-heading = Install
tools-browser-store-button = Chrome Web Store
tools-browser-download-button = Download .zip
tools-browser-download-note = The .zip is for installing unpacked: on Linux, or in Chrome's developer mode. On Windows and macOS, Chrome only installs extensions from the store.
tools-browser-steps-heading = Setup
tools-browser-step-install = Install the extension from the Chrome Web Store. It needs Chrome 127 or newer.
tools-browser-step-pair = Open the extension's settings and add this AIplane's address. Chrome then asks for permission on that address; allowing it completes the pairing.
tools-browser-step-switch-on = Come back to this page and click Switch on, or use the extension's icon in the toolbar. The first time, Chrome asks for access to websites.
tools-browser-step-ask = Ask in a conversation, for example: "Open our internal wiki and summarise the onboarding page."
tools-browser-unpacked-heading = Install unpacked
tools-browser-unpacked-steps = Unzip the download, open chrome://extensions, turn on Developer mode, click Load unpacked and choose the unzipped folder.
tools-browser-origin-label = This AIplane's address
tools-browser-copy-origin = Copy
tools-browser-copied = Copied
tools-browser-notes-heading = Good to know
tools-browser-note-window = The assistant works in its own window, in a tab group named "Assistant". While the extension is on, its icon is green and Chrome shows a bar saying the browser is being debugged.
tools-browser-note-open = It only works while the conversation is open in a tab. Switch it off from the extension's icon when you are done.
tools-browser-note-injection = Pages the assistant reads can contain text aimed at it. In the extension's settings you can limit it to sites you approve.

tool-forward-request-title = Ask another agent
tool-forward-request-description = Sends an explicit task to another personal agent. Both agents must permit the request; the receiving agent uses its own tools and approval rules.
