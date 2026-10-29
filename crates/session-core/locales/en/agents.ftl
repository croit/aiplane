# Strings the agent output filter (#89) puts into an answer on its own.

agent-output-withheld = I cannot give that answer because it mentioned details I could not verify for you. Please rephrase your question or contact support.
agent-output-redacted = [removed]
agent-error-name-taken = A personal agent with that name already exists in your account. Choose another name.
agent-error-name-invalid = Enter a name of up to 80 characters without control characters.
agent-error-active-limit = Your active personal agent limit has been reached. Deactivate an agent before creating another.
agent-error-not-found = That agent is not available to you.
agent-error-id-missing = The request is missing an agent ID.
agent-memory-error-kind = Choose preference, project, or fact for the memory kind.
agent-memory-error-empty = The memory content must not be empty.
agent-memory-error-id-missing = The request is missing an agent ID or memory ID.

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
agent-error-inactive = This agent is inactive. Activate it before starting a conversation.
agent-action-chat = Chat
agent-action-chat-opening = Opening chat…
agent-activity-session-started = Started an agent chat.

agent-workspace-error-unavailable = Persistent agent storage is unavailable. Ask your administrator to check the sandbox runner.
agent-workspace-error-conflict = The workspace is busy or has changed. Reload and retry when the agent is idle.
agent-workspace-error-not-found = That workspace entry was not found. Reload the directory.
agent-workspace-error-too-large = The file or directory exceeds the workspace limit. Use a smaller file or directory.
agent-workspace-error-invalid = The workspace request is invalid. Check the path and try again.
agent-workspace-error-failed = The workspace operation failed. Reload and try again.

agent-workspace-heading = Workspace
agent-workspace-intro = Files in this workspace persist across agent runs. Download files to inspect them.
agent-workspace-reload = Reload workspace
agent-workspace-unavailable = Persistent storage is unavailable. Ask your administrator to check the sandbox runner.
agent-workspace-busy = This agent is using its workspace. Files may change; upload, folder creation and deletion are available when it is idle.
agent-workspace-empty = This folder is empty.
agent-workspace-root = Workspace root
agent-workspace-folder = Folder
agent-workspace-size-missing = Size unavailable
agent-workspace-modified = Modified { $when }
agent-workspace-upload = Upload file
agent-workspace-file = File
agent-workspace-upload-limit = Choose a file of at most 50 MiB.
agent-workspace-create-folder = Create folder
agent-workspace-folder-name = Folder name
agent-workspace-delete = Delete
agent-workspace-delete-confirm = Delete “{ $name }”? Folders must be empty.
agent-workspace-more = Load more files
agent-workspace-error-load = The workspace could not be loaded. Reload it and try again.
agent-workspace-error-save = The workspace change could not be saved. Reload the workspace and try again.
agent-workspace-notice-saved = Workspace updated.
