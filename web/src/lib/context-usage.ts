/**
 * How full a conversation's context is.
 *
 * Two facts from two places: the size the upstream last measured, which the
 * session snapshot carries, and what the model the composer is pointed at can
 * hold, which comes with the model list. The fold point is that model's
 * ceiling — the window minus the room an answer needs — resolved by the same
 * code the compaction trigger runs, so the line turns amber exactly when the
 * next answer would fold.
 */
export interface ContextUsage {
	/** The measured prompt size, in tokens. */
	tokens: number;
	/** The model's context window; null when the model has none to name. */
	window: number | null;
	/** Whole percent of the window in use; null without a known window. */
	percent: number | null;
	/** The size at which older turns are folded into a checkpoint; null when compaction is off. */
	compactsAt: number | null;
	/** At or past the fold point: the next answer summarises the older turns. */
	folding: boolean;
}

/**
 * The conversation's context, or `null` when there is nothing to show.
 *
 * Nothing to show is: no answer has reported its prompt size yet (the upstream
 * may not send usage frames at all), or the conversation has never been sent
 * anywhere. A brand-new chat must not advertise "0 %".
 */
export function contextUsage(input: {
	usedTokens: number | null;
	window: number | null | undefined;
	compactsAt: number | null | undefined;
}): ContextUsage | null {
	const { usedTokens } = input;
	if (usedTokens === null || usedTokens <= 0) return null;
	const window = input.window != null && input.window > 0 ? input.window : null;
	const compactsAt = input.compactsAt != null && input.compactsAt > 0 ? input.compactsAt : null;
	return {
		tokens: usedTokens,
		window,
		percent: window === null ? null : Math.round((usedTokens / window) * 100),
		compactsAt,
		folding: compactsAt !== null && usedTokens >= compactsAt
	};
}
