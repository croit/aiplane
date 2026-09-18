/**
 * Settings: pair a gateway, choose site access, forget a trusted site.
 *
 * Pairing does two things that have to happen together — ask Chrome for
 * permission on the gateway's origin, and register the content script there.
 * A gateway saved without the permission would look paired and do nothing; a
 * content script registered without it cannot run. Both are undone on removal.
 */
import {
	SITE_ACCESS_MODES,
	isSafeGatewayOrigin,
	pairableOrigins,
	withDefaults
} from './policy.js';

const listEl = document.getElementById('gateways');
const approvedEl = document.getElementById('approved');
const approvedBlock = document.getElementById('approved-block');
const siteInput = document.getElementById('site-url');
const siteStatus = document.getElementById('site-status');
const inputEl = document.getElementById('gateway-url');
const statusEl = document.getElementById('pair-status');

async function load() {
	const { settings } = await chrome.storage.local.get(['settings']);
	return withDefaults(settings);
}

async function save(next) {
	await chrome.storage.local.set({ settings: next });
	render();
}

// Same scheme as the service worker's, which re-registers these after an
// extension reload. They must agree or a pairing would be registered twice.
function contentScriptId(origin) {
	return `gw-${origin.replace(/[^a-z0-9]/gi, '-')}`;
}

async function pair(rawUrl) {
	let origin;
	try {
		origin = new URL(rawUrl).origin;
	} catch {
		statusEl.textContent = 'That is not a URL.';
		return;
	}
	if (!isSafeGatewayOrigin(origin)) {
		// Plain http would put the whole session on the wire; the loopback
		// interface is the exception, because that is where people develop
		// against the gateway — including `mise run dev`, which serves
		// http://127.0.0.1:8080 and would otherwise be unpairable.
		statusEl.textContent = 'Use https, or http on 127.0.0.1 / localhost for development.';
		return;
	}

	// On loopback this is both spellings of the same server — see
	// `pairableOrigins`. One prompt covers them, and the content script is
	// registered for each, because a match pattern is per origin.
	const origins = pairableOrigins(origin);
	const patterns = origins.map((o) => `${o}/*`);
	const granted = await chrome.permissions.request({ origins: patterns }).catch(() => false);
	if (!granted) {
		statusEl.textContent = 'Chrome did not grant access to that origin, so nothing was paired.';
		return;
	}

	await chrome.scripting
		.registerContentScripts(
			origins.map((o) => ({
				id: contentScriptId(o),
				matches: [`${o}/*`],
				js: ['src/content.js'],
				runAt: 'document_idle'
			}))
		)
		.catch(() => {
			// Already registered from an earlier pairing of the same origin.
		});

	const settings = await load();
	await save({ ...settings, gateways: [...new Set([...settings.gateways, ...origins])] });
	statusEl.textContent =
		origins.length > 1
			? `Paired ${origins.join(' and ')} — they are the same server. Open either and ` +
				`switch the extension on from the toolbar.`
			: `Paired ${origin}. Open it and switch the extension on from the toolbar.`;
	inputEl.value = '';
}

async function unpair(origin) {
	await chrome.scripting
		.unregisterContentScripts({ ids: [contentScriptId(origin)] })
		.catch(() => {});
	await chrome.permissions.remove({ origins: [`${origin}/*`] }).catch(() => {});
	const settings = await load();
	await save({ ...settings, gateways: settings.gateways.filter((g) => g !== origin) });
}

/**
 * Approve one site for `approved_sites` mode.
 *
 * Here rather than mid-batch on purpose: `chrome.permissions.request` only
 * works inside a user gesture, so a service worker reacting to a message from a
 * web page can never ask. Approving is therefore something the user does up
 * front, from a page with a button on it.
 */
async function approveSite(rawUrl) {
	let origin;
	try {
		origin = new URL(rawUrl).origin;
	} catch {
		siteStatus.textContent = 'That is not a URL.';
		return;
	}
	const granted = await chrome.permissions.request({ origins: [`${origin}/*`] }).catch(() => false);
	siteStatus.textContent = granted
		? `Approved ${origin}.`
		: 'Chrome did not grant access to that site.';
	if (granted) siteInput.value = '';
	render();
}

/** Sites Chrome currently reports a grant for, minus the paired gateways. */
async function approvedSites(settings) {
	const held = await chrome.permissions.getAll();
	return (held.origins ?? [])
		.map((pattern) => pattern.replace(/\/\*$/, ''))
		.filter((origin) => !origin.includes('*') && !settings.gateways.includes(origin));
}

async function render() {
	const settings = await load();

	listEl.replaceChildren(
		...(settings.gateways.length === 0
			? [row('No gateway paired yet.', null)]
			: settings.gateways.map((origin) => row(origin, () => unpair(origin))))
	);

	approvedBlock.hidden = settings.siteAccess !== 'approved_sites';
	if (!approvedBlock.hidden) {
		const sites = await approvedSites(settings);
		approvedEl.replaceChildren(
			...(sites.length === 0
				? [row('No site approved yet — nothing can run.', null)]
				: sites.map((origin) =>
						row(origin, async () => {
							await chrome.permissions.remove({ origins: [`${origin}/*`] }).catch(() => {});
							render();
						})
					))
		);
	}

	for (const radio of document.querySelectorAll('input[name="site-access"]')) {
		radio.checked = radio.value === settings.siteAccess;
		radio.onchange = async () => {
			if (!SITE_ACCESS_MODES.includes(radio.value)) return;
			const current = await load();
			await save({ ...current, siteAccess: radio.value });
		};
	}
}

function row(label, onRemove) {
	const li = document.createElement('li');
	const span = document.createElement('span');
	span.textContent = label;
	li.append(span);
	if (onRemove) {
		const button = document.createElement('button');
		button.textContent = 'Remove';
		button.addEventListener('click', onRemove);
		li.append(button);
	}
	return li;
}

document.getElementById('add-gateway').addEventListener('click', () => pair(inputEl.value.trim()));
document.getElementById('add-site').addEventListener('click', () => approveSite(siteInput.value.trim()));

render();
