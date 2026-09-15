/**
 * Sticky-bottom rules for the conversation transcript.
 *
 * The transcript is a plain scroll box that nothing ever scrolled: a sent
 * message landed below the fold, and a streaming reply grew out of sight,
 * so the reader had to chase both with the scrollbar.
 *
 * The rule people expect from a chat window: while you are at (or within
 * arm's reach of) the end, the view follows what arrives. Scroll up into the
 * conversation to read and the following stops at once — the reply keeps
 * arriving below you, and nothing yanks the page out from under you. Scroll
 * back to the end and it follows again. None of that is a mode the reader
 * sets; it is only ever a function of where they are, which is why it lives
 * here as one, testable, DOM-free.
 */

/** Everything a scroll box tells us about where it sits. */
export interface ScrollMetrics {
	scrollTop: number;
	scrollHeight: number;
	clientHeight: number;
}

/**
 * How close to the end still counts as "at the end".
 *
 * Viewport-relative, with a floor for short windows: a fixed pixel budget is
 * either a hair-trigger on a tall screen or half the window on a phone. A
 * tenth of the viewport is roughly one line of reading slack — enough that
 * nudging the wheel while the last line lands does not switch following off,
 * far too little to be mistaken for going back to read.
 */
export function stickyThreshold(clientHeight: number): number {
	return Math.max(64, Math.round(clientHeight * 0.1));
}

/** Pixels of unseen content below the viewport. */
export function distanceFromBottom({
	scrollTop,
	scrollHeight,
	clientHeight
}: ScrollMetrics): number {
	return Math.max(0, scrollHeight - clientHeight - scrollTop);
}

/**
 * Whether the view should follow new content, given where it sits now.
 *
 * A transcript shorter than its window has no bottom to leave, so it always
 * follows — otherwise the first reply in a new conversation would arrive
 * with following already off.
 */
export function shouldFollow(metrics: ScrollMetrics): boolean {
	return distanceFromBottom(metrics) <= stickyThreshold(metrics.clientHeight);
}

/** The scroll position that puts the end of the transcript in view. */
export function endScrollTop({ scrollHeight, clientHeight }: ScrollMetrics): number {
	return Math.max(0, scrollHeight - clientHeight);
}

/**
 * Whether to keep following, given a scroll event and where the box sat
 * before it.
 *
 * Reading `shouldFollow` straight off every scroll event looks right and is
 * not: a scroll event reports the position at the time it is *delivered*,
 * which for the scroll we performed ourselves can be a frame after the reply
 * grew past it. The view was at the end when we put it there and a hundred
 * pixels short of the new end by the time the event arrived, so following
 * switched itself off one message after every send.
 *
 * Direction settles it. Only the reader ever scrolls *back*, so that is the
 * only thing that stops the view following. Anything moving the other way can
 * re-arm following when it reaches the end, but never cancel it.
 */
export function nextFollow(following: boolean, previousTop: number, metrics: ScrollMetrics): boolean {
	if (metrics.scrollTop < previousTop) return shouldFollow(metrics);
	return following || shouldFollow(metrics);
}
