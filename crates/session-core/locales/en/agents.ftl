# Strings the agent output filter (#89) puts into an answer on its own.

agent-output-withheld = I cannot give that answer because it mentioned details I could not verify for you. Please rephrase your question or contact support.
agent-output-redacted = [removed]
dot-error-name-taken = A personal Dot with that name already exists in your account. Choose another name.
dot-error-name-invalid = Enter a name of up to 80 characters without control characters.
dot-error-active-limit = Your active personal Dot limit has been reached. Deactivate a Dot before creating another.
dot-error-not-found = That Dot is not available to you.
dot-error-id-missing = The request is missing a Dot ID.
dot-memory-error-kind = Choose preference, project, or fact for the memory kind.
dot-memory-error-empty = The memory content must not be empty.
dot-memory-error-id-missing = The request is missing a Dot ID or memory ID.

# What a website visitor reads next to the secure field when an identity
# verifier (#95) asks for the code it sent. Worded the same whether or not
# the address is registered, so it reveals neither.
agent-verifier-code-sent = If this address is registered with us, we have sent a code to it. Enter the code here.
agent-verifier-code-again = Please enter the code we sent you.

# What a website visitor reads when the public agent endpoint refuses a
# request (#92): the agent's budget is spent, or a rate limit was hit.
agent-embed-unavailable = This assistant is temporarily unavailable. Please try again later or use the website's other contact options.
agent-embed-rate-limited = You are sending messages faster than this assistant accepts them. Please wait { $seconds } seconds and try again.

# What a website visitor reads when they answer a paused conversation and the
# answer is refused: the request is for staff, nothing is waiting, or a
# message already waits behind the pending request.
agent-embed-decision-for-staff = This request is waiting for a member of staff. You will see the answer here once they have decided.
agent-embed-not-waiting = The assistant is not waiting for an answer from you right now. Please reload the conversation.
agent-embed-message-waiting = Your previous message is still waiting for the assistant. Please wait for its answer before sending another one.

# What a website visitor reads next to the secure field when an external
# agent behind an A2A route (#101) asks for structured input to continue.
agent-a2a-input-required = A partner service needs one more detail to continue. Enter it here; it goes to that service only.
dot-error-inactive = This Dot is inactive. Activate it before starting a conversation.
dot-action-chat = Chat
dot-action-chat-opening = Opening chat…
dot-activity-session-started = Started a Dot chat.

dot-workspace-error-unavailable = Persistent Dot storage is unavailable. Ask your administrator to check the sandbox runner.
dot-workspace-error-conflict = The workspace is busy or has changed. Reload and retry when the Dot is idle.
dot-workspace-error-not-found = That workspace entry was not found. Reload the directory.
dot-workspace-error-too-large = The file or directory exceeds the workspace limit. Use a smaller file or directory.
dot-workspace-error-invalid = The workspace request is invalid. Check the path and try again.
dot-workspace-error-failed = The workspace operation failed. Reload and try again.

dot-workspace-heading = Workspace
dot-workspace-intro = Files in this workspace persist across Dot runs. Download files to inspect them.
dot-workspace-reload = Reload workspace
dot-workspace-unavailable = Persistent storage is unavailable. Ask your administrator to check the sandbox runner.
dot-workspace-busy = This Dot is using its workspace. Files may change; upload, folder creation and deletion are available when it is idle.
dot-workspace-empty = This folder is empty.
dot-workspace-root = Workspace root
dot-workspace-folder = Folder
dot-workspace-size-missing = Size unavailable
dot-workspace-modified = Modified { $when }
dot-workspace-upload = Upload file
dot-workspace-file = File
dot-workspace-upload-limit = Choose a file of at most 50 MiB.
dot-workspace-create-folder = Create folder
dot-workspace-folder-name = Folder name
dot-workspace-delete = Delete
dot-workspace-delete-confirm = Delete “{ $name }”? Folders must be empty.
dot-workspace-more = Load more files
dot-workspace-error-load = The workspace could not be loaded. Reload it and try again.
dot-workspace-error-save = The workspace change could not be saved. Reload the workspace and try again.
dot-workspace-notice-saved = Workspace updated.
