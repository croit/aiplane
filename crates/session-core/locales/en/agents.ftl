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
