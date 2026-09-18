/**
 * Content script on a paired gateway origin.
 *
 * It is a wire, nothing more: page → service worker → page. It is registered
 * at runtime (`chrome.scripting.registerContentScripts`) only for origins the
 * user paired, because a self-hosted gateway can live on any hostname and
 * `externally_connectable.matches` is fixed at publish time — it cannot name a
 * host we only learn about after install.
 *
 * What this file must never grow: a decision. Everything it forwards arrives
 * from the page, and script on the page is exactly what an attacker who got
 * into the gateway would have. The service worker re-checks all of it. In
 * particular `activate` does not switch anything on — it only asks the
 * extension to put its own question in front of the user.
 *
 * It is also injected straight into tabs that were already open when the
 * extension reloaded, so it has to survive running twice in one document: a
 * second listener would answer every request twice, and nothing downstream
 * would notice, because the page resolves on the first reply it sees.
 */

(() => {
	const INSTALLED = '__gatewayBrowserBridgeInstalled';
	if (window[INSTALLED]) return { installed: false };
	window[INSTALLED] = true;

	const REQUEST_TYPE = 'gateway-browser-request';
	const RESPONSE_TYPE = 'gateway-browser-response';
	const PING_TYPE = 'gateway-browser-ping';
	const PONG_TYPE = 'gateway-browser-pong';
	const ACTIVATE_TYPE = 'gateway-browser-activate';
	const ACTIVATE_RESULT_TYPE = 'gateway-browser-activate-result';
	const STATE_TYPE = 'gateway-browser-state';

	window.addEventListener('message', (event) => {
		// Only this document. `source` is set by the browser, so an embedded frame
		// cannot pose as the top-level page.
		if (event.source !== window || event.origin !== window.location.origin) return;
		const data = event.data;
		if (!data || typeof data !== 'object') return;

		if (data.type === PING_TYPE) {
			chrome.runtime.sendMessage({ type: 'status' }, (reply) => {
				// Reaching this listener at all is what tells the page the
				// extension is installed. `armed` is the separate question of
				// whether it may act, and answering "no" quickly beats leaving a
				// tool parked for two minutes.
				const alive = !chrome.runtime.lastError;
				window.postMessage(
					{ type: PONG_TYPE, present: alive, armed: alive && reply?.armed === true },
					window.location.origin
				);
			});
			return;
		}

		if (data.type === ACTIVATE_TYPE) {
			// Asks the extension to show its own switch. It cannot arm anything:
			// that needs a click inside the extension's UI, which is the one
			// thing script on this page cannot produce. `auto` marks the
			// unprompted offer a page makes on load, which the extension makes
			// at most once so a reload does not reopen the popup every time.
			chrome.runtime.sendMessage({ type: 'activate', auto: data.auto === true }, (reply) => {
				const body = chrome.runtime.lastError ? { opened: false } : (reply ?? { opened: false });
				window.postMessage({ type: ACTIVATE_RESULT_TYPE, ...body }, window.location.origin);
			});
			return;
		}

		if (data.type === REQUEST_TYPE) {
			// Keyed by request, not by turn: one turn can have several batches in
			// flight, because the gateway runs a round's tool calls concurrently.
			const requestId = String(data.request_id ?? '');
			chrome.runtime.sendMessage(
				{ type: 'run', request_id: requestId, actions: data.actions },
				(reply) => {
					const body = chrome.runtime.lastError
						? { error: chrome.runtime.lastError.message ?? 'extension unavailable' }
						: (reply ?? { error: 'the extension returned nothing' });
					window.postMessage(
						{ type: RESPONSE_TYPE, request_id: requestId, ...body },
						window.location.origin
					);
				}
			);
		}
	});

	// The switch lives in the extension, so the page cannot see it being
	// flipped. Without this push the page would have to poll to notice, and
	// the user would watch their own click take a second to register.
	chrome.runtime.onMessage.addListener((message) => {
		if (message?.type === 'state') {
			window.postMessage(
				{ type: STATE_TYPE, present: true, armed: message.armed === true },
				window.location.origin
			);
		}
	});

	// `chrome.scripting.executeScript` hands this back, which is how the worker
	// tells "I just repaired this tab" from "it was already fine".
	return { installed: true };
})();
