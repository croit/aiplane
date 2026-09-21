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
