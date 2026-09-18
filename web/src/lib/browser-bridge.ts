/**
 * Relay between a `browser_action` event and the paired browser extension.
 *
 * The gateway cannot reach the user's browser — no tunnel, no port, no static
 * address. So `browser_control` travels the only channel that always exists:
 * out on this page's event stream, and back on an authenticated POST. This
 * module is the hinge in the middle.
 *
 * The page is a **courier, not a participant**. It does not inspect, reorder,
 * filter or approve actions, and it never decides that something is safe. All
 * of that lives in the extension, on the other side of the trust boundary,
 * because script running on this origin — an XSS, a bad dependency, a
 * compromised gateway — is indistinguishable from this module. A check written
 * here is one an attacker simply skips.
 *
 * What the page *does* owe the gateway is an answer. A tool parked on the hub
 * would otherwise hang for two minutes before timing out, so "no extension
 * here" is reported immediately and explicitly.
 */

import type { BrowserAction, BrowserActionEvent, BrowserFeedbackBody } from './chat-protocol';

/** Message types on the `window.postMessage` channel to the content script. */
const REQUEST_TYPE = 'gateway-browser-request';
const RESPONSE_TYPE = 'gateway-browser-response';
const PING_TYPE = 'gateway-browser-ping';
const PONG_TYPE = 'gateway-browser-pong';
const ACTIVATE_TYPE = 'gateway-browser-activate';
const ACTIVATE_RESULT_TYPE = 'gateway-browser-activate-result';
const STATE_TYPE = 'gateway-browser-state';

/**
 * How long to wait for the extension to answer a probe. Generous enough for a
 * suspended MV3 service worker to wake, short enough that a user without the
 * extension isn't left staring at nothing.
 */
const PROBE_TIMEOUT_MS = 1_500;

/**
 * How long to wait for a batch. Must stay under the tool's own 120s wait, or
 * the gateway gives up first and the user's approval lands on a dead request.
 */
const BATCH_TIMEOUT_MS = 110_000;

/**
 * How long a client with no extension waits before reporting that.
 *
 * Every open client of a conversation receives the same `browser_action` event
 * — a phone and a laptop on the same chat both relay it. The hub resolves on
 * the first reply, so without this delay the phone's instant "no extension"
 * beats the laptop's extension and the model is told nothing ran while the
 * laptop is still doing it. Losing that race is the one outcome worth
 * engineering against, because it is the model reporting the opposite of what
 * happened.
 *
 * Note what does **not** decide this: whether the tab is in front. A visibility
 * gate looks like the obvious way to pick the user's "real" client, and it is
 * wrong here — the assistant's own working tab takes focus, so the chat tab is
 * hidden *precisely when* the extension is doing what it was asked to do. A
 * gate on that made a lone client answer nothing at all, and the tool sat out
 * its full two-minute wait before reporting that the browser never replied.
 * Having an armed extension is the discriminator; being in front is not.
 *
 * It still does not fix the case where the user takes longer than this to
 * approve a write on another device; see docs/browser-control.md.
 */
const NO_EXTENSION_GRACE_MS = 8_000;

/** What comes back: the feedback body minus the id the caller supplies. */
export type BridgeResult = Omit<BrowserFeedbackBody, 'request_id'>;

/**
 * The slice of `window` this module uses.
 *
 * Structural rather than `Window` so the tests can drive it with a plain
 * object — these suites run under `node --test` with no DOM, and a bridge that
 * could only be exercised in a browser would not be exercised at all.
 */
export interface BridgeTarget {
	addEventListener(type: 'message', handler: (event: BridgeMessageEvent) => void): void;
	removeEventListener(type: 'message', handler: (event: BridgeMessageEvent) => void): void;
	postMessage(message: unknown, targetOrigin: string): void;
	location: { origin: string };
}

export interface BridgeMessageEvent {
	source: unknown;
	origin: string;
	data: unknown;
}

type Poster = (turnId: string, body: BrowserFeedbackBody) => Promise<unknown>;

/**
 * What the caller supplies.
 *
 * `post` is required and injected rather than imported: keeping this module
 * free of `api.ts` keeps it free of `fetch` and of SvelteKit, which is what
 * lets the whole relay be exercised under `node --test` with no DOM. The one
 * real caller passes `api.browserFeedback`, so the POST still goes through the
 * SPA's single JSON path.
 *
 * The timeouts are seams for the tests: the batch wait is 110 seconds, and a
 * suite that had to sit through one to prove a mismatched reply is ignored
 * would cost more than the test is worth.
 */
export interface BridgeDeps {
	post: Poster;
	target?: BridgeTarget;
	probeTimeoutMs?: number;
	batchTimeoutMs?: number;
	graceMs?: number;
}

function currentWindow(): BridgeTarget | undefined {
	return typeof window === 'undefined' ? undefined : (window as unknown as BridgeTarget);
}

/**
 * Ask the extension to run `actions`, then report the outcome to the gateway.
 *
 * Always posts exactly once — including when no extension answers, when it
 * throws, and when it never replies. A batch that silently produced nothing
 * would leave the model parked on a turn that is already over.
 */
export async function handleBrowserAction(
	request: BrowserActionEvent,
	deps: BridgeDeps
): Promise<BridgeResult> {
	const post = deps.post;
	const target = deps.target ?? currentWindow();

	let result: BridgeResult;
	if (!target || !(await extensionPresent(target, deps.probeTimeoutMs))) {
		result = { no_extension: true };
		// Give a better-equipped client time to answer first.
		await sleep(deps.graceMs ?? NO_EXTENSION_GRACE_MS);
	} else {
		result = await runBatch(
			target,
			request.request_id,
			request.actions,
			deps.batchTimeoutMs ?? BATCH_TIMEOUT_MS
		);
	}

	try {
		await post(request.turn_id, { ...result, request_id: request.request_id });
	} catch {
		// The gateway times the tool out on its own; there is nothing useful to
		// show the user here, and throwing would take down the subscriber that
		// delivers the rest of the conversation.
	}
	return result;
}

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * What the extension on this page is: absent, installed-but-off, or ready.
 *
 * The distinction matters because "installed but off" is the case worth
 * offering something for. Silence and "not armed" used to collapse into one
 * boolean, which meant the page could not tell a user who has the extension
 * and needs one click from a user who has never installed it.
 */
export interface ExtensionStatus {
	present: boolean;
	armed: boolean;
}

const ABSENT: ExtensionStatus = { present: false, armed: false };

export function extensionStatus(
	target?: BridgeTarget,
	timeoutMs = PROBE_TIMEOUT_MS
): Promise<ExtensionStatus> {
	const win = target ?? currentWindow();
	if (!win) return Promise.resolve(ABSENT);
	return ask<ExtensionStatus>(
		win,
		{ type: PING_TYPE },
		(data) => {
			const pong = data as { type?: string; present?: boolean; armed?: boolean };
			if (pong?.type !== PONG_TYPE) return undefined;
			// A reply at all means a content script is there. `present` is sent
			// explicitly by current versions; an older one that only sent
			// `armed` is still installed, so absence of the field is not absence
			// of the extension.
			return { present: pong.present !== false, armed: pong.armed === true };
		},
		timeoutMs,
		ABSENT
	);
}

/**
 * Whether an extension is paired *and* armed for this page right now.
 *
 * Not exported: `extensionStatus` is what a new caller wants, because the
 * interesting case — installed but switched off — is the one this boolean
 * throws away.
 */
async function extensionPresent(
	target?: BridgeTarget,
	timeoutMs = PROBE_TIMEOUT_MS
): Promise<boolean> {
	// An extension that is installed but not armed for this session is not
	// available, and saying so early beats a two-minute timeout.
	return (await extensionStatus(target, timeoutMs)).armed;
}

/**
 * Ask the extension to put its own switch in front of the user.
 *
 * Deliberately not "switch it on": arming happens on a click inside the
 * extension's UI, which script on this origin cannot synthesise, and that is
 * the whole reason a compromised gateway cannot arm itself. All this page can
 * do is raise the question — and it costs the user exactly one click, because
 * Chrome 127 dropped the gesture requirement on `openPopup` and the extension's
 * popup can therefore appear on its own.
 *
 * `auto` marks the offer this page makes unprompted on load. The extension
 * honours it once per browser session, so closing the popup without switching
 * on is not undone by the next reload.
 *
 * `opened` comes back false when Chrome would not open the popup — an
 * unfocused window, an older Chrome, or an offer already made — and the caller
 * should then show something itself and point at the toolbar icon, which the
 * extension has badged.
 */
export function requestActivation(
	options: { auto?: boolean; target?: BridgeTarget; timeoutMs?: number } = {}
): Promise<{ opened: boolean }> {
	const win = options.target ?? currentWindow();
	if (!win) return Promise.resolve({ opened: false });
	return ask<{ opened: boolean }>(
		win,
		{ type: ACTIVATE_TYPE, auto: options.auto === true },
		(data) => {
			const reply = data as { type?: string; opened?: boolean };
			return reply?.type === ACTIVATE_RESULT_TYPE ? { opened: reply.opened === true } : undefined;
		},
		options.timeoutMs ?? PROBE_TIMEOUT_MS,
		{ opened: false }
	);
}

/**
 * Watch the switch.
 *
 * The extension pushes when it is armed or disarmed, so the page reflects a
 * click the user made in the extension's popup immediately instead of polling
 * for it. Returns the unsubscribe.
 */
export function onExtensionState(
	handler: (status: ExtensionStatus) => void,
	target?: BridgeTarget
): () => void {
	const win = target ?? currentWindow();
	if (!win) return () => {};
	const listener = (event: BridgeMessageEvent) => {
		if (!sameDocument(event, win)) return;
		const data = event.data as { type?: string; armed?: boolean };
		if (data?.type !== STATE_TYPE) return;
		handler({ present: true, armed: data.armed === true });
	};
	win.addEventListener('message', listener);
	return () => win.removeEventListener('message', listener);
}

/**
 * Only messages from this very document.
 *
 * `source` is set by the browser, so a frame cannot claim to be the top
 * document. This keeps an embedded third-party iframe from answering on the
 * extension's behalf — it does **not** defend against script on our own origin,
 * which is the extension's job and stated as such in the module docs.
 */
function sameDocument(event: BridgeMessageEvent, win: BridgeTarget): boolean {
	return event.source === win && event.origin === win.location.origin;
}

/** Hand one batch to the extension and wait for its verdict. */
function runBatch(
	target: BridgeTarget,
	requestId: string,
	actions: BrowserAction[],
	timeoutMs: number
): Promise<BridgeResult> {
	return ask<BridgeResult>(
		target,
		{ type: REQUEST_TYPE, request_id: requestId, actions },
		(data) => {
			const reply = data as ({ type?: string; request_id?: string } & BridgeResult) | null;
			if (reply?.type !== RESPONSE_TYPE) return undefined;
			// A reply for another request belongs to a different in-flight batch
			// — and one turn can legitimately have several.
			if (reply.request_id !== requestId) return undefined;
			return {
				results: reply.results,
				error: reply.error,
				refused: reply.refused,
				no_extension: reply.no_extension
			};
		},
		timeoutMs,
		{ error: 'the browser extension did not answer in time' }
	);
}

/**
 * Post one message to the content script and settle on the first reply the
 * caller accepts, or on the timeout.
 *
 * One scaffold for both round trips: the listener/timer teardown is exactly
 * where a leak or a double-resolve would hide, and two hand-written copies of
 * it means fixing one and not the other.
 */
function ask<T>(
	target: BridgeTarget,
	outgoing: unknown,
	accept: (data: unknown) => T | undefined,
	timeoutMs: number,
	onTimeout: T
): Promise<T> {
	return new Promise((resolve) => {
		let settled = false;
		let timer: ReturnType<typeof setTimeout>;
		const done = (value: T) => {
			if (settled) return;
			settled = true;
			clearTimeout(timer);
			target.removeEventListener('message', onMessage);
			resolve(value);
		};
		const onMessage = (event: BridgeMessageEvent) => {
			if (!sameDocument(event, target)) return;
			const accepted = accept(event.data);
			if (accepted !== undefined) done(accepted);
		};
		target.addEventListener('message', onMessage);
		target.postMessage(outgoing, target.location.origin);
		timer = setTimeout(() => done(onTimeout), timeoutMs);
	});
}
