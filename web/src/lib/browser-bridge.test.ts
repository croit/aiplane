/**
 * Unit tests for the browser-extension relay (run by `mise run test-web` via
 * `node --test`, Node's native type stripping — no DOM).
 *
 * The property that matters most here is not "it relays actions" but **it
 * always answers**. A `browser_control` tool parks on a hub until this page
 * posts back; every path that ends without a POST is a turn that hangs for two
 * minutes and then reports a timeout the user cannot explain.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
	extensionStatus,
	handleBrowserAction,
	onExtensionState,
	requestActivation,
	type BridgeMessageEvent,
	type BridgeResult,
	type BridgeTarget
} from './browser-bridge.ts';
import type {
	BrowserAction,
	BrowserActionEvent,
	BrowserFeedbackBody
} from './chat-protocol.ts';

const ORIGIN = 'https://gw.example.com';

/**
 * A stand-in for `window` plus a scripted extension on the other end.
 *
 * `reply` decides what the fake extension does with each outgoing message:
 * return a value to post back, or `null` to stay silent (the "no extension"
 * case).
 */
function fakeWindow(reply: (message: Record<string, unknown>) => unknown | null): {
	target: BridgeTarget;
	sent: Record<string, unknown>[];
	deliver: (data: unknown) => void;
} {
	const handlers: ((event: BridgeMessageEvent) => void)[] = [];
	const sent: Record<string, unknown>[] = [];
	/** An unprompted message from the extension — it pushes its switch. */
	const deliver = (data: unknown) => {
		for (const handler of [...handlers]) handler({ source: target, origin: ORIGIN, data });
	};
	const target: BridgeTarget = {
		addEventListener(_type, handler) {
			handlers.push(handler);
		},
		removeEventListener(_type, handler) {
			const i = handlers.indexOf(handler);
			if (i >= 0) handlers.splice(i, 1);
		},
		postMessage(message) {
			const msg = message as Record<string, unknown>;
			sent.push(msg);
			const answer = reply(msg);
			if (answer === null || answer === undefined) return;
			// Deliver asynchronously, like the real message channel.
			queueMicrotask(() => {
				for (const handler of [...handlers]) {
					handler({ source: target, origin: ORIGIN, data: answer });
				}
			});
		},
		location: { origin: ORIGIN }
	};
	return { target, sent, deliver };
}

/** An extension that is present, armed, and answers the batch with `result`. */
function workingExtension(result: Record<string, unknown>) {
	return fakeWindow((msg) => {
		if (msg.type === 'gateway-browser-ping') return { type: 'gateway-browser-pong', armed: true };
		if (msg.type === 'gateway-browser-request') {
			return { type: 'gateway-browser-response', request_id: msg.request_id, ...result };
		}
		return null;
	});
}

function recorder(): {
	posts: [string, BrowserFeedbackBody][];
	post: (t: string, b: BrowserFeedbackBody) => Promise<unknown>;
} {
	const posts: [string, BrowserFeedbackBody][] = [];
	return {
		posts,
		post: async (turnId, body) => {
			posts.push([turnId, body]);
			return { ok: true };
		}
	};
}

const READ: BrowserAction[] = [{ action: 'read_page' }];

/** A batch as the event stream delivers it. */
function request(actions: BrowserAction[] = READ): BrowserActionEvent {
	return { type: 'browser_action', turn_id: 't1', request_id: 'r1', actions };
}



test('a batch reaches the extension and its results are posted back', async () => {
	const { target, sent } = workingExtension({ results: [{ title: 'Example' }] });
	const { posts, post } = recorder();

	const result = await handleBrowserAction(request(), { post, target, graceMs: 5 });

	const handed = sent.find((m) => m.type === 'gateway-browser-request');
	assert.ok(handed, 'the batch must be handed to the extension');
	assert.equal(handed.request_id, 'r1');
	assert.deepEqual(handed.actions, READ);

	assert.deepEqual(result.results, [{ title: 'Example' }]);
	assert.equal(posts.length, 1, 'exactly one report per batch');
	assert.deepEqual(posts[0][1].results, [{ title: 'Example' }]);
	assert.equal(posts[0][0], 't1', 'the turn id authorises the POST');
	assert.equal(posts[0][1].request_id, 'r1', 'the request id routes it to the right tool');
});

test('actions are relayed verbatim — the page is a courier, not a filter', async () => {
	// A write action must reach the extension unchanged and un-approved. If the
	// page ever started vetoing actions, the real enforcement point (the
	// extension) would be the only one still being tested, and this side would
	// quietly become a second, weaker policy.
	const risky: BrowserAction[] = [
		{ action: 'navigate', url: 'https://bank.example.com/transfer' },
		{ action: 'type_text', ref: 'e3', text: '1000', submit: true }
	];
	const { target, sent } = workingExtension({ results: [] });
	const { post } = recorder();

	await handleBrowserAction(request(risky), { post, target, graceMs: 5 });

	const handed = sent.find((m) => m.type === 'gateway-browser-request');
	assert.deepEqual(handed?.actions, risky);
});

test('no extension is reported at once, not left to time out', async () => {
	// Nothing answers the probe.
	const { target, sent } = fakeWindow(() => null);
	const { posts, post } = recorder();

	const result = await handleBrowserAction(request(), { post, target, probeTimeoutMs: 20, graceMs: 5 });

	assert.equal(result.no_extension, true);
	assert.equal(posts.length, 1);
	assert.equal(posts[0][1].no_extension, true);
	assert.ok(
		!sent.some((m) => m.type === 'gateway-browser-request'),
		'a batch must not be sent to an extension that is not there'
	);
});

test('an installed but unarmed extension counts as no extension', async () => {
	// Arming is a click on the extension's own icon; until then it must not
	// act, and the model needs to hear that rather than wait.
	const { target } = fakeWindow((msg) =>
		msg.type === 'gateway-browser-ping' ? { type: 'gateway-browser-pong', armed: false } : null
	);
	const { posts, post } = recorder();

	const result = await handleBrowserAction(request(), { post, target, graceMs: 5 });

	assert.equal(result.no_extension, true);
	assert.equal(posts[0][1].no_extension, true);
});

test('a refusal is relayed as a refusal', async () => {
	const { target } = workingExtension({ refused: 'the user declined the click' });
	const { posts, post } = recorder();

	const result = await handleBrowserAction(request(), { post, target, graceMs: 5 });

	assert.equal(result.refused, 'the user declined the click');
	assert.equal(posts[0][1].refused, 'the user declined the click');
});

test('messages from another frame are ignored', async () => {
	// An embedded third-party iframe must not be able to answer on the
	// extension's behalf. `source` is set by the browser, so this is a real
	// boundary — unlike anything we could check about the message body.
	const handlers: ((event: BridgeMessageEvent) => void)[] = [];
	const target: BridgeTarget = {
		addEventListener: (_t, h) => void handlers.push(h),
		removeEventListener: (_t, h) => {
			const i = handlers.indexOf(h);
			if (i >= 0) handlers.splice(i, 1);
		},
		postMessage: () => {
			queueMicrotask(() => {
				for (const handler of [...handlers]) {
					// Right shape, wrong sender.
					handler({
						source: { imposter: true },
						origin: ORIGIN,
						data: { type: 'gateway-browser-pong', armed: true }
					});
				}
			});
		},
		location: { origin: ORIGIN }
	};
	const { posts, post } = recorder();

	const result = await handleBrowserAction(request(), { post, target, probeTimeoutMs: 20, graceMs: 5 });

	assert.equal(result.no_extension, true, 'a foreign frame must not pass as the extension');
	assert.equal(posts.length, 1);
});

test('a reply for a different batch does not settle this one', async () => {
	// Two batches can be in flight for ONE turn, because the runner executes a
	// round's tool calls concurrently. Answering the wrong one would hand a
	// model the page that another call asked for.
	const { target } = fakeWindow((msg) => {
		if (msg.type === 'gateway-browser-ping') return { type: 'gateway-browser-pong', armed: true };
		if (msg.type === 'gateway-browser-request') {
			return {
				type: 'gateway-browser-response',
				request_id: 'a-different-batch',
				results: [{ x: 1 }]
			};
		}
		return null;
	});
	const { post } = recorder();

	const pending = handleBrowserAction(request(), { post, target, batchTimeoutMs: 60, graceMs: 5 });
	// Nothing settles on the foreign reply; the batch ends on its own timeout,
	// reported as an error rather than as someone else's page.
	const settled = await Promise.race([
		pending.then((r) => r),
		new Promise<'pending'>((r) => setTimeout(() => r('pending'), 20))
	]);
	assert.equal(settled, 'pending', 'a mismatched request id must not resolve the batch');
	const finished = await pending;
	assert.ok(finished.error, 'it must end as a timeout, not as the other batch\'s results');
	assert.equal(finished.results, undefined);
});

test('a failing POST does not throw at the caller', async () => {
	// The subscriber that calls this also delivers the rest of the
	// conversation; an exception here would take the transcript down with it.
	const { target } = workingExtension({ results: [] });
	const failing = async () => {
		throw new Error('network down');
	};

	await assert.doesNotReject(() => handleBrowserAction(request(), { post: failing, target, graceMs: 5 }));
});

test('a backgrounded tab still relays — it is usually the right client', async () => {
	// The assistant's working tab takes focus, so the chat tab is hidden exactly
	// when the extension is busy doing what it was asked. A visibility gate here
	// made the only client answer nothing at all, and the tool sat out its full
	// two-minute wait before reporting that the browser never replied.
	const { target, sent } = workingExtension({ results: [{ title: 'Example' }] });
	const { posts, post } = recorder();

	const result = await handleBrowserAction(request(), { post, target, graceMs: 5 });

	assert.deepEqual(result.results, [{ title: 'Example' }]);
	assert.equal(posts.length, 1, 'it must answer, whether or not the tab is in front');
	assert.ok(sent.some((m) => m.type === 'gateway-browser-request'));
});

test('"no extension" waits before answering, so an equipped client can win', async () => {
	// The grace period is the whole mitigation for the multi-client race; without
	// it this POST is instant and always beats a real extension elsewhere.
	const { target } = fakeWindow(() => null);
	const { posts, post } = recorder();

	const started = Date.now();
	await handleBrowserAction(request(), {
		post,
		target,

		probeTimeoutMs: 10,
		graceMs: 120
	});

	assert.equal(posts[0][1].no_extension, true);
	assert.ok(Date.now() - started >= 100, 'it must hold back before claiming nothing ran');
});

test('an installed-but-off extension is told apart from no extension at all', async () => {
	// The distinction the activation offer rests on. Collapsed into one boolean
	// these two look identical, and the page then either advertises an
	// extension to somebody who never installed one, or stays silent for
	// somebody who is one click away from having it work.
	const off = fakeWindow((msg) =>
		msg.type === 'gateway-browser-ping'
			? { type: 'gateway-browser-pong', present: true, armed: false }
			: null
	);
	assert.deepEqual(await extensionStatus(off.target, 50), { present: true, armed: false });

	const absent = fakeWindow(() => null);
	assert.deepEqual(await extensionStatus(absent.target, 20), { present: false, armed: false });

	const on = fakeWindow((msg) =>
		msg.type === 'gateway-browser-ping'
			? { type: 'gateway-browser-pong', present: true, armed: true }
			: null
	);
	assert.deepEqual(await extensionStatus(on.target, 50), { present: true, armed: true });
});

test('activating asks for the switch, never for being switched on', async () => {
	// The page may raise the question; arming needs a gesture inside the
	// extension's own UI. If this ever starts sending something the worker acts
	// on directly, a compromised gateway can arm itself and the toolbar switch
	// stops being a boundary.
	const { target, sent } = fakeWindow((msg) =>
		msg.type === 'gateway-browser-activate'
			? { type: 'gateway-browser-activate-result', opened: true }
			: null
	);

	assert.deepEqual(await requestActivation({ target, timeoutMs: 50 }), { opened: true });
	assert.deepEqual(
		sent.map((m) => m.type),
		['gateway-browser-activate'],
		'nothing but the request to show the switch'
	);
});

test('a popup Chrome refused to open is reported rather than assumed shown', async () => {
	// `openPopup` only works in a focused window. The banner falls back to
	// pointing at the toolbar icon, which it can only do if it hears about it.
	const refused = fakeWindow((msg) =>
		msg.type === 'gateway-browser-activate'
			? { type: 'gateway-browser-activate-result', opened: false }
			: null
	);
	assert.deepEqual(
		await requestActivation({ target: refused.target, timeoutMs: 50 }),
		{ opened: false }
	);

	const silent = fakeWindow(() => null);
	assert.deepEqual(
		await requestActivation({ target: silent.target, timeoutMs: 20 }),
		{ opened: false }
	);
});

test('the extension pushes its switch, so the page never polls for it', async () => {
	// The user clicks "switch on" inside the extension's popup. Nothing about
	// that reaches this page unless the worker says so, and a page that polled
	// would lag its own user's click.
	const { target, deliver } = fakeWindow(() => null);
	const seen: { present: boolean; armed: boolean }[] = [];
	const unwatch = onExtensionState((status) => seen.push(status), target);

	deliver({ type: 'gateway-browser-state', armed: true });
	deliver({ type: 'gateway-browser-state', armed: false });
	unwatch();
	deliver({ type: 'gateway-browser-state', armed: true });

	assert.deepEqual(seen, [
		{ present: true, armed: true },
		{ present: true, armed: false }
	]);
});

test('an unrelated message never passes for a state push', async () => {
	// This listener sits on `window.message`, where every library on the page
	// also posts. Anything but our own type has to be ignored.
	const { target, deliver } = fakeWindow(() => null);
	const seen: unknown[] = [];
	onExtensionState((status) => seen.push(status), target);

	deliver({ type: 'webpack-hmr' });
	deliver('a string');
	deliver(null);

	assert.deepEqual(seen, []);
});

test('the offer a page makes on load is marked as such', async () => {
	// The extension honours an unprompted offer once per browser session. A
	// page that did not mark it would have its popup reopened on every reload,
	// including after the user closed it — which is what people uninstall an
	// extension over.
	const { target, sent } = fakeWindow((msg) =>
		msg.type === 'gateway-browser-activate'
			? { type: 'gateway-browser-activate-result', opened: true }
			: null
	);

	await requestActivation({ auto: true, target, timeoutMs: 50 });
	await requestActivation({ target, timeoutMs: 50 });

	assert.deepEqual(
		sent.map((m) => m.auto),
		[true, false],
		'the load-time offer and the button press must be distinguishable'
	);
});
