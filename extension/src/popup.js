/**
 * The arm / disarm switch, and the log of what was done.
 *
 * This popup is the only place the extension can be switched on. That is the
 * point: the click is a user gesture delivered to the extension, which no web
 * page can synthesise — so "armed" is a state a compromised gateway cannot
 * enter on the user's behalf.
 *
 * It is also where the **host permission** is taken, for the same reason:
 * `chrome.permissions.request` only works inside a gesture. Asking from the
 * service worker when a batch arrives is rejected by Chrome every time, so the
 * grant has to be collected here, at the moment the user says "yes, act in my
 * browser".
 */
import { BROAD_ORIGINS, withDefaults } from './policy.js';

const titleEl = document.getElementById('title');
const statusEl = document.getElementById('status');
const toggleEl = document.getElementById('toggle');
const activityEl = document.getElementById('activity');

document.getElementById('options').addEventListener('click', (event) => {
	event.preventDefault();
	chrome.runtime.openOptionsPage();
});

/** Origin of the tab in front of the user, if any. */
async function currentOrigin() {
	const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
	try {
		return new URL(tab?.url ?? '').origin;
	} catch {
		return null;
	}
}

async function render() {
	// Opening the popup also repairs a pairing whose content script was lost to
	// an extension reload — see `ensureContentScripts`. It is the one surface
	// that still works when the page cannot reach us at all.
	const [{ settings }, origin, repair] = await Promise.all([
		chrome.storage.local.get(['settings']),
		currentOrigin(),
		chrome.runtime.sendMessage({ type: 'repair' }).catch(() => null)
	]);
	const conf = withDefaults(settings);
	const gateways = conf.gateways;
	// `status` carries the activity log, so the worker reads session storage once.
	const status = await chrome.runtime.sendMessage({ type: 'status', origin }).catch(() => null);
	const activity = status?.activity ?? [];

	if (gateways.length === 0) {
		titleEl.textContent = 'Not paired yet';
		statusEl.textContent = 'Add your gateway in Settings before this can do anything.';
		toggleEl.hidden = true;
	} else if (!origin || !gateways.includes(origin)) {
		titleEl.textContent = 'Not your gateway';
		statusEl.textContent =
			`This page is ${origin ?? 'not a paired gateway'}. Open ` +
			`${gateways.join(' or ')} and switch it on there.`;
		toggleEl.hidden = true;
	} else {
		const armedHere = status?.armed === true;
		titleEl.textContent = armedHere ? 'On for this conversation' : 'Off';
		statusEl.textContent = armedHere
			? `${origin} may act in this browser until you switch it off or close Chrome.`
			: `${origin} is paired. Switch on to let it act in this browser.`;
		toggleEl.hidden = false;
		toggleEl.textContent = armedHere ? 'Switch off' : 'Switch on';
		if (repair?.repaired?.length) {
			// Worth saying out loud: the user just watched it not work. No reload
			// is asked for — the worker injects into the open tabs itself.
			statusEl.textContent = `Reconnected to ${repair.repaired.join(', ')}. ${
				statusEl.textContent
			}`;
		}
		toggleEl.classList.toggle('on', !armedHere);
		toggleEl.onclick = () => (armedHere ? disarm() : arm(origin, conf));
	}

	activityEl.replaceChildren(
		...activity.slice(0, 12).map((entry) => {
			const li = document.createElement('li');
			// textContent throughout: these strings come from pages we visited.
			li.textContent = `${entry.at.slice(11, 19)} · ${entry.event}${
				entry.action ? ` ${entry.action}` : ''
			}${entry.reason ? ` — ${entry.reason}` : ''}`;
			return li;
		})
	);
}

/**
 * Switch on — and, in the default "all sites" mode, collect the broad host
 * permission while we still have the gesture that lets us ask.
 *
 * Asked here rather than at install so the warning appears in context, when
 * the user has just said they want the assistant to act in their browser.
 */
async function arm(origin, conf) {
	if (conf.siteAccess !== 'approved_sites') {
		const granted = await chrome.permissions
			.request({ origins: BROAD_ORIGINS })
			.catch(() => false);
		if (!granted) {
			statusEl.textContent =
				'Chrome did not grant access to websites, so nothing was switched on. ' +
				'You can instead approve individual sites under Settings.';
			return;
		}
	}
	await chrome.runtime.sendMessage({ type: 'arm', origin });
	render();
}

async function disarm() {
	await chrome.runtime.sendMessage({ type: 'disarm' });
	render();
}

render();
