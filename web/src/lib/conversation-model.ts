/**
 * The model a conversation was last sent to: the assistant turn records the
 * picker id the message was submitted with (never a route's resolved target),
 * so the newest turn naming one is the conversation's last used model.
 */
export function lastUsedModel(turns: { model: string | null }[]): string | null {
	for (let index = turns.length - 1; index >= 0; index--) {
		const model = turns[index].model;
		if (model) return model;
	}
	return null;
}

/**
 * The model the picker opens a conversation on. `unavailable` names a last
 * used model the offered list no longer contains, so the switch is said out
 * loud instead of happening silently. An empty list leaves the picker in its
 * free-text fallback, where the last used model is kept as typed.
 */
export function openingModel(
	lastUsed: string | null,
	offered: { id: string }[]
): { model: string; unavailable: string | null } {
	if (offered.length === 0) return { model: lastUsed ?? '', unavailable: null };
	if (lastUsed && offered.some((candidate) => candidate.id === lastUsed)) {
		return { model: lastUsed, unavailable: null };
	}
	return { model: offered[0].id, unavailable: lastUsed };
}
