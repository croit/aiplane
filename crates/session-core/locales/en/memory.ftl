# Strings owned by `gateway/src/rama_server/pages/memory.rs` — the
# per-user /memory page: inspect, add, edit, and delete the structured
# memories the assistant keeps about you.

memory-heading = Memory
memory-description = What the assistant remembers about you, grouped by kind. Add, edit, or delete entries here — it's your account's memory and fully under your control. Turn the capability on or off on the Tools page.

memory-add-heading = Add a memory
memory-kind-aria = Memory kind
memory-content-placeholder = e.g. Prefers answers in metric units

memory-empty = Nothing here yet.
memory-save-button = Save
memory-delete-title = Delete memory

# SPA-only: the Svelte /memory page's inline add/edit form.
memory-kind-preference = Preferences
memory-kind-project = Project context
memory-kind-fact = Facts
memory-content-label = Content
memory-add-button = Remember
memory-delete-confirm = Delete this memory?

# The per-category Add button in each card header; it opens the add dialog
# with that card's kind already selected.
memory-add-short = Add

# One line under each card heading saying how that kind reaches the
# assistant: preferences ride in the system context of every conversation,
# while project notes and facts wait to be looked up with `recall`. The
# difference changes which bucket a user files something in, and nothing
# else on the page reveals it.
memory-kind-preference-hint = Always in context — sent with every conversation from the first message, so the assistant applies them without being asked.
memory-kind-project-hint = Fetched on demand — the assistant looks these up when the conversation touches your work.
memory-kind-fact-hint = Fetched on demand — the assistant looks these up when they become relevant.
