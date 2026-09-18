/**
 * Tests for the page↔worker wire (run by `mise run test-extension`).
 *
 * `content.js` touches `chrome.*`, so unlike `policy.js` it cannot simply be
 * imported. It is a classic script rather than a module, though, which means a
 * `vm` context with a stub `window` and `chrome` runs it exactly as Chrome
 * does — including handing back the completion value that
 * `chrome.scripting.executeScript` reports.
 *
 * What is pinned here is one property with a real cost behind it: **it must
 * survive being run twice in the same document**. The extension injects this
 * file directly into tabs that were already open when it reloaded, because a
 * content-script *registration* only applies to future page loads and the tab
 * in front of the user would otherwise stay mute. Injecting into a tab that
 * already has the script is the normal case for that, and a second listener
 * would answer every request twice.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createContext, runInContext } from 'node:vm';

const SOURCE = readFileSync(new URL('./content.js', import.meta.url), 'utf8');
const ORIGIN = 'https://gw.example.com';

/**
 * One document, with the two globals the content script uses.
 *
 * `posted` is what the page would see; `runs` is what reached the worker.
 */
function document_() {
	const listeners = [];
	const runtimeListeners = [];
	const posted = [];
	const runs = [];

	const window = {
		location: { origin: ORIGIN },
		addEventListener(type, handler) {
			if (type === 'message') listeners.push(handler);
		},
		postMessage(data) {
			posted.push(data);
		}
	};
	const chrome = {
		runtime: {
			lastError: undefined,
			sendMessage(message, callback) {
				runs.push(message);
				// The worker answers asynchronously, like the real channel.
				queueMicrotask(() => callback({ ok: true, armed: true, opened: true }));
			},
			onMessage: {
				addListener(handler) {
					runtimeListeners.push(handler);
				}
			}
		}
	};

	const context = createContext({ window, chrome, queueMicrotask });
	return {
		/** Run the script in this document, as Chrome would. */
		inject: () => runInContext(SOURCE, context),
		/** A `window.postMessage` from the page itself. */
		fromPage: (data) => {
			for (const handler of [...listeners]) handler({ source: window, origin: ORIGIN, data });
		},
		/** A `chrome.tabs.sendMessage` from the worker. */
		fromWorker: (message) => {
			for (const handler of [...runtimeListeners]) handler(message);
		},
		listeners,
		posted,
		runs
	};
}

const flush = () => new Promise((resolve) => queueMicrotask(resolve));

/**
 * Cross the realm boundary.
 *
 * Objects the script creates belong to the `vm` context, so they carry that
 * realm's `Object.prototype` and `deepStrictEqual` rejects them as a different
 * type however identical their contents. Chrome's own message channel
 * structured-clones everything anyway, so this is also what the real page
 * receives.
 */
const plain = (value) => JSON.parse(JSON.stringify(value));

test('a second injection into the same page installs nothing', async () => {
	// The reload path: the extension re-registers *and* injects into the tabs
	// already open. A tab that still had the script from before would otherwise
	// end up with two of everything.
	const doc = document_();

	assert.deepEqual(plain(doc.inject()), { installed: true });
	assert.deepEqual(plain(doc.inject()), { installed: false }, 'the second run must be a no-op');
	assert.equal(doc.listeners.length, 1, 'one message listener, not two');

	doc.fromPage({ type: 'gateway-browser-request', request_id: 'r1', actions: [] });
	await flush();
	await flush();

	assert.equal(doc.runs.length, 1, 'the batch must reach the worker once');
	assert.equal(doc.posted.length, 1, 'and be answered once');
});

test('the completion value is what tells a repaired tab from a healthy one', () => {
	// `executeScript` reports it, and the worker uses it to decide whether it
	// actually reconnected anything — which is what the popup then says out
	// loud. Claiming a repair that did not happen is worse than saying nothing.
	const fresh = document_();
	assert.equal(fresh.inject().installed, true);
	assert.equal(fresh.inject().installed, false);
});

test('a ping answers whether the extension is there and whether it may act', async () => {
	// Two separate facts. Folding them into one is what left the page unable to
	// offer the switch to somebody who has the extension but has not turned it
	// on.
	const doc = document_();
	doc.inject();

	doc.fromPage({ type: 'gateway-browser-ping' });
	await flush();
	await flush();

	assert.deepEqual(plain(doc.posted), [
		{ type: 'gateway-browser-pong', present: true, armed: true }
	]);
});

test('activating asks the worker to show its switch and nothing more', async () => {
	// The page is allowed to raise the question. It must never be able to send
	// something that arms the extension outright.
	const doc = document_();
	doc.inject();

	doc.fromPage({ type: 'gateway-browser-activate' });
	await flush();
	await flush();

	// `auto` marks the offer a page makes on load, which the extension honours
	// once per session. Anything the page did not send is false, so a page that
	// omits it gets the asked-for behaviour rather than a silent no-op.
	assert.deepEqual(plain(doc.runs), [{ type: 'activate', auto: false }]);
	assert.deepEqual(plain(doc.posted), [
		{ type: 'gateway-browser-activate-result', ok: true, armed: true, opened: true }
	]);
});

test('a message from an embedded frame is not this page speaking', () => {
	// `source` is set by the browser, so a third-party iframe cannot pose as
	// the top document — but only if we check it.
	const doc = document_();
	doc.inject();

	for (const handler of doc.listeners) {
		handler({ source: {}, origin: ORIGIN, data: { type: 'gateway-browser-ping' } });
		handler({ source: {}, origin: 'https://evil.example', data: { type: 'gateway-browser-ping' } });
	}

	assert.deepEqual(plain(doc.runs), [], 'nothing from a frame may reach the worker');
});

test('the worker can push its switch to the page', () => {
	// How the chat page notices the user clicking "switch on" in the popup,
	// without polling for it.
	const doc = document_();
	doc.inject();

	doc.fromWorker({ type: 'state', armed: true });
	doc.fromWorker({ type: 'something-else' });

	assert.deepEqual(plain(doc.posted), [
		{ type: 'gateway-browser-state', present: true, armed: true }
	]);
});
