# Start using AIplane

AIplane gives you browser conversations and access to the models, tools and integrations your organization makes available. This guide assumes an operator has installed AIplane and given you its address. Installation and model configuration are operator tasks.

## Sign in and send your first message

1. Open your organization's AIplane address.
2. Use the sign-in page and complete the configured identity provider's login.
3. AIplane opens a conversation. The home page and `/chat` both lead to the chat surface; there is no separate dashboard at `/`.
4. Choose an available model in the conversation header. On a narrow screen, the selectors move to a full-width row below the conversation actions.
5. Type a message and press **Enter**, or select **Send**. Use **Shift+Enter** for a newline.
6. Read the reply as it arrives. Tool activity and requests for your input appear in the conversation.

The model list reflects the installation's configuration and your permissions. A model missing from that list is a question for your administrator. The model, transcription and spoken-reply voice selectors remain available on narrow screens; they move below the conversation actions.

Continue with [Conversations](chat.md) to manage messages, sharing and exports, or [Files and documents](files-and-canvas.md) to work with attachments.

## Find your work

The sidebar contains conversations and navigation to tools, agents, schedules, webhooks, inbox and usage. Administrative sections depend on your permissions. On a narrow screen, open the navigation menu first.

Use the conversation search button to search your history. Pin a conversation using its star button to keep it easy to find. AIplane opens on a new chat. Use **New Chat** at the top of the sidebar, or the new-conversation button, to start a separate topic. Each new chat is listed with your conversations; while it has no messages, pressing **New Chat** again opens that same empty chat instead of adding another. Delete a conversation through its delete button and confirm the operation.

Opening another conversation restores its saved transcript and live state. Closing a browser tab is not the same operation as stopping a turn: use the conversation's **Stop** button when you want to cancel work.

## Adjust the interface

Use the language picker for English, German, French, Spanish, Russian or Chinese. The theme button switches between light and dark appearance. The account menu leads to settings and sign-out.

## If the first message fails

- **Login repeats or fails:** ask the operator to inspect identity-provider configuration and the reported error.
- **No usable model:** ask for the relevant model permission or backend configuration. Tools and models are granted separately.
- **Limit exceeded:** check [Usage and tokens](account-and-usage.md), then ask the administrator about your applicable limits.
- **Backend or model error:** retain the displayed error and the selected model when reporting the problem. Retrying can create another metered request.
