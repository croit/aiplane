/**
 * Service worker: the part that actually touches the user's tabs.
 *
 * Everything that can say no lives here or in `policy.js`, never in the page
 * and never in the gateway. The order of checks below is the trust model
 * written out:
 *
 *   1. is the sender a gateway *the user paired*      (not: one that says it is)
 *   2. is the extension armed for this session        (a click on our icon)
 *   3. does the batch parse into steps we implement   (unknown ⇒ refuse)
 *   4. do we hold host permission for the target      (Chrome's grant, not ours)
 *   5. has the user approved this on that domain      (our own dialog)
 *
 * Arming is step 2 because it is the one signal a web page cannot produce: a
 * click delivered to the extension's own action button. Until it happens the
 * extension answers "not armed" and does nothing else.
 *
 * **State lives in `chrome.storage.session`, not in module scope.** An MV3
 * service worker is torn down after ~30s idle — shorter than a model takes to
 * think — so anything held in a variable is gone by the time the batch arrives.
 */

import * as cdp from './cdp.js';
import { evaluateBatch, isPairedGateway, permissionsFor } from './policy.js';

const MAX_ACTIVITY = 50;

/**
 * Toolbar state: green while the extension may act, grey while it may not.
 *
 * Worth its own code because "is this thing on?" is the question the user asks
 * every single time, and until now the only way to answer it was to open the
 * popup. The colour carries the state; the badge repeats it in words for anyone
 * who cannot rely on the colour, and the title spells out which gateway it is
 * armed for.
 */
async function showState(armed) {
	const state = armed ? 'on' : 'off';
	const sizes = [16, 32, 48, 128];

	// Every call is best-effort. The icon is a convenience; arming is not, and
	// the two used to share a fate — `setIcon` rejecting (an extension package
	// loaded before the icons existed, which is exactly what a developer
	// reloading sees) threw straight out of the message handler and the switch
	// never flipped.
	const path = Object.fromEntries(sizes.map((n) => [n, `icons/${state}-${n}.png`]));
	const ok = await chrome.action
		.setIcon({ path })
		.then(() => true)
		.catch(() => false);
	if (!ok) {
		// Draw it instead of fetching it. Works whatever is or is not in the
		// package, and costs nothing for two rounded squares.
		await chrome.action
			.setIcon({ imageData: Object.fromEntries(sizes.map((n) => [n, drawIcon(n, armed)])) })
			.catch(() => {});
	}

	await chrome.action.setBadgeText({ text: armed ? 'on' : '' }).catch(() => {});
	await chrome.action.setBadgeBackgroundColor({ color: '#16a34a' }).catch(() => {});
	await chrome.action
		.setTitle({
			title: armed ? `Browser control — on for ${armed.origin}` : 'Browser control — off'
		})
		.catch(() => {});
}

/**
 * The same mark the PNGs show, drawn in the worker.
 *
 * A fallback, for when `setIcon` cannot load the files — which is exactly what
 * a developer sees after reloading an unpacked extension whose package
 * predates the artwork. It has to *be* the same mark, or the fallback is a
 * second icon nobody recognises; the proportions below are the ones in
 * `icons/render.py`, and the two change together.
 */
function drawIcon(size, armed) {
	const canvas = new OffscreenCanvas(size, size);
	const ctx = canvas.getContext('2d');

	ctx.fillStyle = '#1D1D1B';
	ctx.beginPath();
	ctx.roundRect(0, 0, size, size, size * 0.25);
	ctx.fill();

	const inset = size * 0.117;
	const mid = size / 2;
	if (armed) {
		// The gateway's accent, bottom-left to top-right as in the favicon.
		const accent = ctx.createLinearGradient(inset, size - inset, size - inset, inset);
		accent.addColorStop(0, '#8E54E9');
		accent.addColorStop(1, '#FCA68C');
		ctx.fillStyle = accent;
	} else {
		ctx.fillStyle = '#6b7280';
	}
	ctx.beginPath();
	ctx.moveTo(mid, inset);
	ctx.lineTo(size - inset, mid);
	ctx.lineTo(mid, size - inset);
	ctx.lineTo(inset, mid);
	ctx.closePath();
	ctx.fill();

	ctx.fillStyle = armed ? '#ffffff' : '#d1d5db';
	ctx.beginPath();
	ctx.arc(mid, mid, size * 0.135, 0, Math.PI * 2);
	ctx.fill();
	return ctx.getImageData(0, 0, size, size);
}

// The worker is torn down and restarted constantly; the icon is per-session
// browser state, so it has to be re-asserted from storage whenever we come
// back rather than only when the user flips the switch.
chrome.runtime.onStartup.addListener(() => revive());
chrome.runtime.onInstalled.addListener(() => revive());

async function revive() {
	const { armed } = await session();
	await showState(armed);
	// A reload gets to ask again. `chrome.storage.session` survives an
	// extension reload — it is cleared when the *browser* closes, not when we
	// do — so without this an offer spent before the reload stays spent, and
	// the one surface that asks the user anything never appears again. That is
	// exactly how this presented: paired, installed, and silent.
	await chrome.storage.session.remove('offered');
	await ensureContentScripts();
}

/**
 * Put the content script back on every paired gateway — both for the pages
 * loaded from now on and for the ones already open.
 *
 * Dynamic registrations do **not** survive an extension update, and reloading
 * an unpacked extension is an update. The pairing itself lives in storage, so
 * everything *looked* right afterwards: the gateway still listed, the icon
 * still green, "armed" still true — and every batch answered "no extension",
 * because the page had no content script to reach us through. The reload the
 * user performs to pick up a fix was itself what broke it.
 *
 * Registering alone is not enough, and that is the part worth remembering: a
 * registration only applies to future page loads, so the tab the user is
 * looking at stays mute until they reload it. Asking them to is a poor answer
 * when we know the tab and can inject into it ourselves.
 *
 * Idempotent from both ends: registered ids are left alone, and the content
 * script refuses to install itself twice in one document.
 */
async function ensureContentScripts() {
	const conf = await settings();
	const gateways = conf.gateways ?? [];
	if (gateways.length === 0) return [];

	const existing = new Set(
		(await chrome.scripting.getRegisteredContentScripts().catch(() => [])).map((s) => s.id)
	);
	const repaired = [];
	for (const origin of gateways) {
		// Only where Chrome still grants us the origin; a revoked permission
		// means the user unpaired it in Chrome's own UI, and re-adding it here
		// would quietly undo that.
		const allowed = await chrome.permissions
			.contains({ origins: [`${origin}/*`] })
			.catch(() => false);
		if (!allowed) continue;

		let touched = false;
		if (!existing.has(contentScriptId(origin))) {
			touched = await chrome.scripting
				.registerContentScripts([
					{
						id: contentScriptId(origin),
						matches: [`${origin}/*`],
						js: ['src/content.js'],
						runAt: 'document_idle'
					}
				])
				.then(() => true)
				.catch(() => false);
		}
		if (await injectInto(origin)) touched = true;
		if (touched) repaired.push(origin);
	}
	if (repaired.length > 0) {
		await note({ event: 'repaired', reason: `reconnected ${repaired.join(', ')}` });
	}
	return repaired;
}

/**
 * Inject into the tabs of one origin that are open right now.
 *
 * Returns whether any tab actually gained the bridge. A tab that already had
 * it answers `installed: false`, so opening the popup a second time does not
 * claim to have fixed something.
 */
async function injectInto(origin) {
	const tabs = await chrome.tabs.query({ url: `${origin}/*` }).catch(() => []);
	let installed = false;
	for (const tab of tabs) {
		if (tab.id === undefined) continue;
		const results = await chrome.scripting
			.executeScript({ target: { tabId: tab.id }, files: ['src/content.js'] })
			.catch(() => []);
		if (results.some((r) => r?.result?.installed)) installed = true;
	}
	return installed;
}

/** Same id scheme as the options page, so the two never register twice. */
function contentScriptId(origin) {
	return `gw-${origin.replace(/[^a-z0-9]/gi, '-')}`;
}

/** Armed state, working tab and its window. Cleared when the browser closes. */
async function session() {
	const stored = await chrome.storage.session.get(['armed', 'workingTabId', 'workingWindowId']);
	return {
		armed: stored.armed ?? null,
		workingTabId: stored.workingTabId ?? null,
		workingWindowId: stored.workingWindowId ?? null
	};
}

async function settings() {
	const stored = await chrome.storage.local.get(['settings']);
	return stored.settings ?? {};
}

async function activityLog() {
	const stored = await chrome.storage.session.get(['activity']);
	return stored.activity ?? [];
}

/**
 * Append entries to the log in one write.
 *
 * Takes a list, not a single entry: a batch produces one entry per action, and
 * a read-modify-write per action turned an eight-step batch into sixteen extra
 * storage round trips — each one deserialising the whole 50-entry log to add a
 * line to it.
 */
async function note(...entries) {
	const stamped = entries.map((entry) => ({ at: new Date().toISOString(), ...entry }));
	const next = [...stamped.reverse(), ...(await activityLog())].slice(0, MAX_ACTIVITY);
	await chrome.storage.session.set({ activity: next });
}

/**
 * Show the extension's own switch, because a gateway page asked us to.
 *
 * A paired page can tell that the extension is installed but off, and offering
 * to turn it on there is a far better first run than hoping the user finds a
 * grey icon in the toolbar. What the page must not get is the switching
 * itself: arming takes a click inside the extension's UI, which is the one
 * gesture script on a page cannot synthesise, and it is where the host
 * permission is collected for the same reason.
 *
 * So this opens the popup and stops. Chrome 127 dropped the user-gesture
 * requirement on `openPopup`, which is what makes the one-click flow possible:
 * the page offers on load, the popup appears, the user clicks "switch on"
 * once. When Chrome refuses anyway — an unfocused window, an older Chrome —
 * we say so, and the page falls back to a banner, with the badge making the
 * toolbar icon hard to miss.
 */
async function offerSwitch(origin, auto, tab) {
	const conf = await settings();
	if (!origin || !(conf.gateways ?? []).includes(origin)) {
		// Only a paired gateway. Anything else has no business asking.
		return { opened: false, paired: false };
	}
	const { armed } = await session();
	if (armed !== null && armed.origin === origin) return { opened: false, armed: true };

	// An unprompted offer is made once per browser session per gateway. The
	// page makes it on every load, and a window that reopens on every reload —
	// including after the user closed it without switching on — is the kind of
	// thing people uninstall an extension over. Asked-for offers always open,
	// because that click *is* the answer to this one.
	const { offered } = await chrome.storage.session.get(['offered']);
	const already = offered ?? [];
	if (auto && already.includes(origin)) {
		// Asked once already, so do not ask again — but leave the badge on.
		// The question is still open, and the toolbar icon is then the only
		// thing still saying so.
		await markQuestionWaiting();
		return { opened: false, paired: true, spent: true };
	}

	// The popup opens over whatever tab is in front, not over the tab that
	// asked. A chat loading in a background tab would otherwise drop it on top
	// of whatever the user is actually reading — which is how this was first
	// noticed, over somebody's YouTube tab. So an unprompted offer waits until
	// its own tab is the one in front, and the page asks again when it becomes
	// visible.
	if (auto && !(await isInFront(tab))) {
		return { opened: false, paired: true, notInFront: true };
	}

	await markQuestionWaiting();

	const { opened, how, reason } = await showSwitch(origin, tab);
	// Only a question that actually reached the user counts as asked. Marking
	// it spent before trying meant one refusal from Chrome burned the offer for
	// the whole browser session, and nothing ever asked again.
	if (auto && opened) {
		await chrome.storage.session.set({ offered: [...already, origin] });
	}
	await note({ event: opened ? `offered via ${how}` : 'offer refused', origin, reason });
	return { opened, paired: true };
}

/**
 * Amber badge: a question waiting, not a state. Deliberately not the green of
 * 'on' — `showState` overwrites it the next time the popup reads the status.
 */
async function markQuestionWaiting() {
	await chrome.action.setBadgeText({ text: '1' }).catch(() => {});
	await chrome.action.setBadgeBackgroundColor({ color: '#d97706' }).catch(() => {});
}

/**
 * Open the extension's own popup — the one anchored to the toolbar icon.
 *
 * Deliberately the only surface. A separate window would always appear and
 * never vanish, which is tempting, but it is not what the toolbar button does
 * and it lands on the desktop like an alert. The popup is the thing people
 * already know from clicking the icon, so that is what the page's offer opens.
 *
 * The complication is that a toolbar popup closes itself the moment it loses
 * focus, and the page that asked for it is usually **still loading** — it takes
 * the focus back and the popup is gone within a frame. `openPopup()` resolves
 * happily either way, which is what made this invisible: the offer was recorded
 * as made, the badge went amber, and nothing had ever been on screen.
 *
 * So: wait for the page to settle, open, then check with `getViews` whether it
 * actually stayed. If it did not, try once more after the load has finished
 * for sure. Failing that we stop and leave the badge on — an offer nobody saw
 * is better than a popup that keeps flickering at them.
 */
async function showSwitch(_origin, tab) {
	let reason = '';
	// First attempt after the page has had a moment to finish taking focus,
	// second well after any load could still be stealing it.
	for (const wait of [500, 1500]) {
		await sleep(wait);
		const asked = await chrome.action
			.openPopup(tab?.windowId === undefined ? {} : { windowId: tab.windowId })
			.then(() => true)
			.catch((err) => {
				// The one thing that explains a failure, and it shows up in the
				// activity log rather than being swallowed.
				reason = String(err?.message ?? err);
				return false;
			});
		if (asked && (await popupSurvived())) return { opened: true, how: 'popup', reason };
		if (asked && !reason) reason = 'the popup closed itself before it could be read';
	}
	return { opened: false, how: 'popup', reason };
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Whether the popup is still on screen shortly after being opened.
 *
 * `getViews` is the only way to ask. When it is unavailable we cannot tell, and
 * the honest answer is "no": retrying costs a flicker, while trusting it costs
 * the user the only prompt they were going to get.
 */
async function popupSurvived() {
	await sleep(400);
	try {
		return (chrome.extension.getViews({ type: 'popup' }) ?? []).length > 0;
	} catch {
		return false;
	}
}

/** Whether this tab is the one the user is looking at right now. */
async function isInFront(tab) {
	if (!tab?.active || tab.windowId === undefined) return false;
	const window = await chrome.windows.get(tab.windowId).catch(() => null);
	return window?.focused === true;
}

/**
 * Tell a gateway's open tabs that the switch moved.
 *
 * The switch is in the extension, so the page cannot observe it. Pushing beats
 * polling here: the user clicks "switch on" and sees the page agree at once.
 */
async function broadcastState(origin, armed) {
	const tabs = await chrome.tabs.query({ url: `${origin}/*` }).catch(() => []);
	await Promise.all(
		tabs.map((tab) =>
			tab.id === undefined
				? null
				: chrome.tabs.sendMessage(tab.id, { type: 'state', armed }).catch(() => {})
		)
	);
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
	handleMessage(message, sender)
		.then(sendResponse)
		.catch((err) => sendResponse({ error: String(err?.message ?? err) }));
	// Keep the message channel open for the async reply.
	return true;
});

async function handleMessage(message, sender) {
	if (message?.type === 'status') {
		const { armed } = await session();
		// The origin to answer about is named by the caller when it can (the
		// popup asks about the tab in front of the user) and taken from the
		// sender otherwise (a content script asks about itself). Deriving it
		// from the sender alone made the popup — which lives on
		// `chrome-extension://` — always read "off", so there was no way to
		// switch the extension off short of closing the browser.
		const origin = message.origin ?? senderOrigin(sender);
		await showState(armed);
		return {
			armed: armed !== null && armed.origin === origin,
			armedFor: armed?.origin ?? null,
			// Carried along: the popup wants both, and asking twice makes the
			// worker read session storage twice for one render.
			activity: await activityLog()
		};
	}
	if (message?.type === 'arm') {
		await chrome.storage.session.set({
			armed: { origin: message.origin, at: Date.now() },
			workingTabId: null,
			workingWindowId: null
		});
		await showState({ origin: message.origin });
		await note({ event: 'armed', origin: message.origin });
		await broadcastState(message.origin, true);
		return { ok: true };
	}
	if (message?.type === 'disarm') {
		// Let go of the debugger session too, so Chrome's "being debugged" bar
		// disappears the moment the user switches the extension off.
		const { armed, workingTabId } = await session();
		if (workingTabId !== null) await cdp.detach(workingTabId);
		await chrome.storage.session.set({ armed: null, workingTabId: null, workingWindowId: null });
		await showState(null);
		await note({ event: 'disarmed' });
		if (armed) await broadcastState(armed.origin, false);
		return { ok: true };
	}
	if (message?.type === 'activity') {
		return { activity: await activityLog() };
	}
	if (message?.type === 'activate') {
		return offerSwitch(senderOrigin(sender), message.auto === true, sender.tab ?? null);
	}
	if (message?.type === 'repair') {
		// The popup asks on every open. It costs one API call and it is the only
		// surface that still works when the content script is gone.
		return { repaired: await ensureContentScripts() };
	}
	if (message?.type === 'run') {
		return runBatch(message, sender);
	}
	return { error: 'unknown message' };
}

function senderOrigin(sender) {
	try {
		return new URL(sender?.url ?? '').origin;
	} catch {
		return null;
	}
}

async function runBatch(message, sender) {
	const origin = senderOrigin(sender);
	const conf = await settings();
	const { armed } = await session();

	// 1. A paired gateway. The message claims an origin; we use the one the
	//    browser attached to the sender instead.
	if (!origin || !isPairedGateway(origin, conf)) {
		await note({ event: 'refused', reason: 'unpaired gateway', origin });
		return { refused: 'this gateway is not paired with the extension' };
	}

	// 2. Armed, for this gateway.
	if (!armed || armed.origin !== origin) {
		return { no_extension: true };
	}

	const tab = await workingTab();
	const verdict = evaluateBatch(message.actions, tab?.url ?? '', conf);
	// 3. Steps we implement, targets we accept.
	if (!verdict.ok) {
		await note({ event: 'refused', reason: verdict.reason });
		return { refused: verdict.reason };
	}

	// 4. Host permission, from Chrome — **checked, never requested**. A request
	//    only works inside a user gesture, and a message from a web page is not
	//    one, so asking here would be rejected every time. The grant is taken
	//    when the user arms the extension or adds a site in settings; a missing
	//    one is reported in terms they can act on rather than as a mystery
	//    failure deep inside `executeScript`.
	const needed = permissionsFor(verdict.origins, conf);
	if (needed.length > 0 && !(await chrome.permissions.contains({ origins: needed }))) {
		await note({ event: 'refused', reason: 'site access missing', origins: verdict.origins });
		return {
			refused:
				conf.siteAccess === 'approved_sites'
					? `${verdict.origins.join(', ')} is not approved yet — add it in the ` +
						`extension's settings`
					: 'the extension does not hold site permission yet — switch it on again ' +
						'from its toolbar icon and accept the permission prompt'
		};
	}

	// There is deliberately no fifth step. A permission for a site is taken to
	// mean the assistant may *work* on that site, not merely look at it: a
	// dialog per click is a dialog nobody reads by the third one, and a consent
	// that is clicked away reflexively protects no one. What carries the weight
	// instead is that the extension does nothing until switched on for this
	// session, that switching it on is a click no page can produce, that the
	// toolbar icon is green the whole time, and that every step is recorded
	// here and in the gateway's audit trail.

	// The tab is resolved once and threaded through: `perform` used to look it
	// up per action, and each lookup was a storage read plus a `tabs.get`.
	const results = [];
	const log = [];
	const tabFor = tabOnce(ensureTab);
	for (const action of message.actions) {
		try {
			results.push(await perform(action, tabFor));
			log.push({ event: 'ran', action: action.action });
		} catch (err) {
			log.push({ event: 'failed', action: action.action, error: String(err?.message ?? err) });
			await note(...log);
			return { error: `${action.action}: ${String(err?.message ?? err)}`, results };
		}
	}
	await note(...log);
	return { results };
}

/** The tab this conversation works in, if it still exists. */
async function workingTab() {
	const { workingTabId } = await session();
	if (workingTabId === null) return null;
	const tab = await chrome.tabs.get(workingTabId).catch(() => null);
	if (tab) return tab;
	await chrome.storage.session.set({ workingTabId: null, workingWindowId: null });
	return null;
}

/** Title and colour of the group the assistant's tabs live in. */
const GROUP_TITLE = 'Assistant';

/**
 * The assistant works in **its own window**, in a labelled tab group, and never
 * takes focus.
 *
 * Three requirements pull against each other here. The user must not have a tab
 * yanked in front of them mid-sentence; they must still be able to see what is
 * being done in their name; and `captureVisibleTab` only ever photographs the
 * *active tab of a window* — point it at the user's window and `screenshot`
 * quietly returns whatever they happened to be reading.
 *
 * A separate, unfocused window satisfies all three: the working tab is the
 * active tab of that window (so captures are of the page we drove), the window
 * sits behind the user's own (so nothing is stolen), and the group's title says
 * whose tabs those are.
 */
async function ensureTab() {
	const existing = await workingTab();
	if (existing) return existing;

	// `focused: false` is the whole point — the window opens behind whatever the
	// user is doing.
	const win = await chrome.windows.create({ url: 'about:blank', focused: false });
	const tab = win.tabs?.[0];
	if (!tab) throw new Error('could not open a window for the assistant');

	// Best-effort: a browser without tab groups still works, it is just less
	// obvious which tabs are the assistant's.
	try {
		const groupId = await chrome.tabs.group({
			tabIds: [tab.id],
			createProperties: { windowId: win.id }
		});
		await chrome.tabGroups.update(groupId, { title: GROUP_TITLE, color: 'blue' });
	} catch {
		// ignored on purpose
	}

	await chrome.storage.session.set({ workingTabId: tab.id, workingWindowId: win.id });
	return tab;
}

/** Resolve the working tab once per batch and reuse it for every step. */
function tabOnce(resolve) {
	let cached = null;
	return async () => {
		if (!cached) cached = await resolve();
		return cached;
	};
}

/** Run one step in the assistant's tab. */
async function perform(action, tabFor) {
	if (action.action === 'list_tabs') {
		const tabs = await chrome.tabs.query({});
		return { tabs: tabs.map((t) => ({ id: t.id, title: t.title, url: t.url })) };
	}

	const tab = await tabFor();
	// Attached once per tab and kept for the session: attaching per action adds
	// a visible bar flicker and ~100ms to every step, and the bar is the honest
	// signal that something is driving this window anyway.
	await cdp.attach(tab.id);

	switch (action.action) {
		case 'navigate': {
			// The load listener is armed *before* the navigation starts;
			// attached afterwards it misses `complete` for a cached page or a
			// fast redirect, and the step then sits out the full fallback.
			const loaded = waitForLoad(tab.id);
			await chrome.tabs.update(tab.id, { url: action.url });
			await loaded;
			return { navigated: action.url };
		}
		case 'go_back': {
			const loaded = waitForLoad(tab.id);
			await chrome.tabs.goBack(tab.id);
			await loaded;
			return { went_back: true };
		}
		case 'screenshot': {
			const dataUrl = await cdp.screenshot(tab.id, { fullPage: Boolean(action.full_page) });
			return { screenshot: dataUrl, full_page: Boolean(action.full_page) };
		}
		case 'set_viewport':
			return await cdp.setViewport(tab.id, {
				width: action.width,
				height: action.height,
				mobile: Boolean(action.mobile)
			});
		case 'click': {
			const at = await cdp.centerOf(tab.id, action.ref);
			await cdp.click(tab.id, at, {
				button: action.button ?? 'left',
				clickCount: action.click_count ?? 1
			});
			return { clicked: action.ref, at };
		}
		case 'hover': {
			const at = await cdp.centerOf(tab.id, action.ref);
			await cdp.moveTo(tab.id, at);
			return { hovered: action.ref };
		}
		case 'drag': {
			const from = await cdp.centerOf(tab.id, action.from);
			const to = await cdp.centerOf(tab.id, action.to);
			await cdp.drag(tab.id, from, to);
			return { dragged: { from: action.from, to: action.to } };
		}
		case 'type_text': {
			// Focus by clicking where the field actually is, so the page sees
			// the same sequence a person produces: pointer, focus, input.
			const at = await cdp.centerOf(tab.id, action.ref);
			await cdp.click(tab.id, at);
			if (action.replace) await cdp.selectAll(tab.id);
			await cdp.insertText(tab.id, action.text);
			if (action.submit) await cdp.pressKey(tab.id, 'Enter');
			return { typed: action.ref, submitted: Boolean(action.submit) };
		}
		case 'press_key':
			await cdp.pressKey(tab.id, action.key, action.modifiers ?? []);
			return { pressed: action.key, modifiers: action.modifiers ?? [] };
		case 'scroll': {
			if (action.ref) {
				const at = await cdp.centerOf(tab.id, action.ref);
				return { scrolled_to: action.ref, at };
			}
			const { result } = await cdp.send(tab.id, 'Runtime.evaluate', {
				expression: 'JSON.stringify({w: innerWidth, h: innerHeight})',
				returnByValue: true
			});
			const view = JSON.parse(result.value);
			// A real wheel event, not `window.scrollBy`: infinite lists and
			// scroll-jacking pages listen for the wheel, not for a scroll
			// position that changed by itself.
			await cdp.wheel(
				tab.id,
				{ x: view.w / 2, y: view.h / 2 },
				action.direction === 'up' ? -view.h * 0.9 : view.h * 0.9
			);
			return { scrolled: action.direction ?? 'down' };
		}
		case 'wait_for':
			return await waitFor(tab.id, action.text, action.timeout_ms ?? 5_000);
		case 'read_page':
		case 'find': {
			const [{ result }] = await chrome.scripting.executeScript({
				target: { tabId: tab.id },
				func: inPage,
				args: [action]
			});
			return result;
		}
		default:
			throw new Error(`unsupported action ${action.action}`);
	}
}

/**
 * Resolve when the tab finishes loading.
 *
 * The listener is armed by the caller *before* the navigation starts: attached
 * afterwards it misses `complete` for a cached page or a fast redirect, and the
 * step then sits out the full fallback. The fallback timer is cleared on the
 * normal path — left pending it keeps the MV3 service worker resident for 15s
 * after every navigation.
 */
function waitForLoad(tabId) {
	return new Promise((resolve) => {
		let timer;
		const done = () => {
			clearTimeout(timer);
			chrome.tabs.onUpdated.removeListener(listener);
			resolve();
		};
		const listener = (id, info) => {
			if (id === tabId && info.status === 'complete') done();
		};
		chrome.tabs.onUpdated.addListener(listener);
		// Never hang the batch on a page that keeps loading forever.
		timer = setTimeout(done, 15_000);
	});
}

/**
 * Wait until `text` is on the page, or simply wait.
 *
 * Needed because a page is usually still assembling itself when `navigate`
 * returns: the model would otherwise read an empty shell, conclude the site is
 * broken, and report that.
 */
async function waitFor(tabId, text, timeoutMs) {
	const deadline = Date.now() + Math.min(timeoutMs, 30_000);
	if (!text) {
		await new Promise((r) => setTimeout(r, Math.min(timeoutMs, 30_000)));
		return { waited_ms: timeoutMs };
	}
	while (Date.now() < deadline) {
		const { result } = await cdp.send(tabId, 'Runtime.evaluate', {
			expression: `(document.body?.innerText ?? '').includes(${JSON.stringify(text)})`,
			returnByValue: true
		});
		if (result?.value === true) return { found: text };
		await new Promise((r) => setTimeout(r, 250));
	}
	throw new Error(`"${text}" did not appear within ${timeoutMs}ms`);
}

/**
 * Runs **inside the page**: reading only.
 *
 * Everything that *acts* moved to the debugger protocol; what is left is
 * building the map the model steers by. Refs are written onto the elements so a
 * later click resolves the element that was read rather than an index into a
 * page that has changed underneath — and so `cdp.centerOf` can find it again by
 * attribute.
 *
 * Self-contained by necessity: `executeScript` serialises this function, so it
 * cannot close over anything above.
 */
function inPage(action) {
	const REF_ATTR = 'data-gw-ref';

	// `offsetParent` is null for every `position: fixed` element, so filtering
	// on it hid exactly the things that block a page — sticky headers, cookie
	// banners, modal dialogs — and nothing could ever dismiss them.
	const visible = (el) => {
		if (typeof el.checkVisibility === 'function') return el.checkVisibility();
		const rect = el.getBoundingClientRect();
		return rect.width > 0 && rect.height > 0;
	};

	const interactive = () =>
		[...document.querySelectorAll('a, button, input, textarea, select, [role="button"]')].filter(
			(el) => visible(el) || el === document.activeElement
		);

	const label = (el) =>
		(
			el.getAttribute('aria-label') ||
			el.innerText ||
			el.value ||
			el.getAttribute('placeholder') ||
			el.getAttribute('name') ||
			''
		)
			.trim()
			.slice(0, 120);

	const tag = (els) =>
		els.map((el, i) => {
			const ref = `e${i + 1}`;
			el.setAttribute(REF_ATTR, ref);
			return { el, ref };
		});

	switch (action.action) {
		case 'read_page': {
			const elements = tag(interactive()).map(({ el, ref }) => ({
				ref,
				tag: el.tagName.toLowerCase(),
				type: el.type ?? null,
				label: label(el)
			}));
			const text = (document.body?.innerText ?? '').slice(0, action.max_chars ?? 8_000);
			return { title: document.title, url: location.href, text, elements };
		}
		case 'find': {
			// Filtered before tagging: labelling every interactive element and
			// writing a ref onto it, only to keep at most twenty, dirtied the
			// DOM for elements that are discarded a line later.
			const needle = String(action.text ?? '').toLowerCase();
			const hits = tag(
				interactive()
					.filter((el) => label(el).toLowerCase().includes(needle))
					.slice(0, 20)
			).map(({ el, ref }) => ({ ref, label: label(el) }));
			const found = (document.body?.innerText ?? '').toLowerCase().includes(needle);
			return { found, matches: hits };
		}
		default:
			throw new Error(`${action.action} is not a page-side action`);
	}
}
