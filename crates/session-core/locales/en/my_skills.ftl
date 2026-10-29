# Strings owned by `gateway/src/rama_server/pages/skills_user.rs` — the
# per-user `/skills` page (a user's own private Agent Skills: upload,
# inline editor, delete, download).

my-skills-heading = My Skills
my-skills-intro = Your own private skills — guidance the chat model loads on demand in your conversations. Only you can see and use them. Upload a .skill archive, or write one inline.

my-skills-upload-button = Upload .skill
my-skills-empty-loaded = No skills yet. Create one, or upload a .skill archive.

my-skills-download-button = Download
my-skills-download-title = Download this skill as a .skill archive
my-skills-delete-button = Delete
my-skills-delete-title = Delete this skill

# SPA-only: the Svelte /skills list (inline body preview + upload toasts).
my-skills-toast-installed = Installed { $name }.
my-skills-delete-confirm = Delete your private skill { $name }?
my-skills-new-button = New skill
my-skills-upload-heading = Upload a skill
my-skills-loaded-heading = Your skills
my-skills-none-yet = None yet
my-skills-empty-not-configured = Skills aren't enabled on this gateway.
my-skills-edit-button = Edit
my-skills-description-heading = Description
my-skills-files-count = { $count } bundled files
my-skills-new-heading = New skill
my-skills-edit-heading = Edit skill
my-skills-editor-hint = A skill is a SKILL.md file: YAML frontmatter (name, description) followed by the instructions. Edit it directly below.
my-skills-save-button = Save
my-skills-cancel-button = Cancel
my-skills-draft-button = Draft from workflow
my-skills-draft-title = Describe a workflow
my-skills-draft-intro = Describe the repeatable task. Your input is sent to the selected model to create an unsaved skill draft. The workflow is not run.
my-skills-draft-name = Workflow name
my-skills-draft-name-placeholder = Review support tickets
my-skills-draft-goal = What should be accomplished?
my-skills-draft-goal-placeholder = Find urgent tickets and flag them for a person.
my-skills-draft-steps = Steps
my-skills-draft-steps-hint = One action per step, in the order it should happen.
my-skills-draft-step-label = Step { $number }
my-skills-draft-step-placeholder = For example: Open each new ticket and check whether it is urgent.
my-skills-draft-remove-step = Remove step
my-skills-draft-add-step = Add step
my-skills-draft-stop = When should the agent stop or ask for approval?
my-skills-draft-stop-placeholder = For example: Always ask before replying to or changing a ticket.
my-skills-draft-model = Model
my-skills-draft-model-placeholder = Choose a model
my-skills-draft-no-models = No chat model is available to you.
my-skills-draft-generating = Creating draft…
my-skills-draft-create = Create draft
my-skills-draft-disabled = Personal skills are not enabled on this gateway.
my-skills-draft-invalid = Provide a name, goal, at least one step, stop or approval conditions, and an available model.
my-skills-draft-model-unavailable = This model is not available to you. Refresh the model list and try again.
my-skills-draft-limit = Your usage limit currently prevents creating this draft.
my-skills-draft-failed = The draft could not be created. Your input is still available.
my-skills-draft-invalid-result = The model did not return a valid skill draft. Please try again.
my-skills-save-name-exists = A skill with this name already exists. Open it to edit or choose a different name.
my-skills-save-name-missing = This skill was removed. Refresh the skill list and create it again.
