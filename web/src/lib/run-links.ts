/**
 * Where a row that fires prompts on its own sends someone who wants to read
 * what it produced.
 *
 * Scheduled actions and webhooks are the same shape: something fires a prompt
 * without a person watching, each fire opens (or continues) a chat, and the
 * list row has to point at the result. One rule for both, because a user who
 * learns it on `/scheduled` should not have to learn it again on `/webhooks`.
 */

export interface RunLinkSource {
	id: string;
	/** The chat the most recent fire opened. */
	last_session_id: string | null;
	/** Recorded fires. */
	run_count: number;
	/** Distinct chats those fires opened — one, when the conversation is reused. */
	chat_count: number;
}

export interface RunLinks {
	/** The single conversation, when there is exactly one. */
	chat: string | null;
	/** The run history, when it holds more than the `chat` link already shows. */
	runs: string | null;
}

/**
 * The links a row offers. `section` is the route prefix the run history lives
 * under (`/scheduled`, `/webhooks`).
 *
 * Something that reuses one conversation has exactly one chat however often
 * it has fired, so the row goes straight into it — a history of identical
 * links would be a detour. Something that opens a fresh chat each time has a
 * list, and that list is the history page. Fires that produced no chat at all
 * (over quota, the model never answered) are still worth seeing, so the
 * history stays reachable whenever it holds anything the chat link does not.
 */
export function runLinks(source: RunLinkSource, section: string): RunLinks {
	const chat = source.chat_count <= 1 && source.last_session_id ? `/chat/${source.last_session_id}` : null;
	const everythingIsInTheChatLink = chat !== null && source.run_count <= 1;
	return {
		chat,
		runs: source.run_count > 0 && !everythingIsInTheChatLink ? `${section}/${source.id}/runs` : null
	};
}
