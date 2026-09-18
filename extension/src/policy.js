/**
 * The extension's own rules about what it will do.
 *
 * Deliberately a **pure module with no `chrome.*` access**, for two reasons:
 * it is the part that decides whether a page gets clicked, so it has to be
 * testable without a browser; and keeping it free of extension APIs makes it
 * obvious that nothing here can be reached or changed from a web page.
 *
 * The rules re-derive everything the gateway already told us. That duplication
 * is the point: the gateway sits on the same side of the trust boundary as an
 * attacker who has script on its origin, so a step it labelled "read" must
 * still be checked here before it is treated as one.
 */

/** Steps that change a page, as opposed to observing it. */
const WRITE_ACTIONS = new Set([
	'navigate',
	'go_back',
	'click',
	'drag',
	'type_text',
	'press_key'
]);

/**
 * Steps that reach beyond the page the assistant is working on.
 *
 * `list_tabs` reports every open tab, so it is not covered by a permission for
 * one site. It is allowed under the blanket grant and refused under
 * `approved_sites`, where the user's answer was explicitly "only these sites".
 */
const CROSS_SITE_ACTIONS = new Set(['list_tabs']);

/** Every step this extension knows how to perform. */
export const KNOWN_ACTIONS = new Set([
	'navigate',
	'go_back',
	'read_page',
	'find',
	'click',
	'hover',
	'drag',
	'type_text',
	'press_key',
	'scroll',
	'screenshot',
	'set_viewport',
	'wait_for',
	'list_tabs'
]);

/**
 * Site-access modes, chosen in the extension's settings.
 *
 * `all_sites` is the default: one broad grant, asked for once, because per-site
 * prompts are what makes this kind of tool unusable for most people. The cost
 * of that default is that the write confirmation below becomes the only thing
 * standing between a prompt-injected page and a click — which is why it cannot
 * be turned off for every domain at once, only remembered per domain.
 */
export const SITE_ACCESS_MODES = ['all_sites', 'approved_sites'];
export const DEFAULT_SETTINGS = {
	siteAccess: 'all_sites',
	/** Gateways allowed to drive this extension, as origins. */
	gateways: []
};

/**
 * Settings with the defaults filled in.
 *
 * One place, because a new key with a default otherwise has to be remembered at
 * every call site, and a missed one reads `undefined` — a `.includes(...)` on
 * it would throw rather than fail closed.
 */
export function withDefaults(settings) {
	return { ...DEFAULT_SETTINGS, ...(settings ?? {}) };
}

/** Whether a step writes. Unknown names count as writes — the safe default. */
export function isWrite(action) {
	return !KNOWN_ACTIONS.has(action?.action) || WRITE_ACTIONS.has(action?.action);
}

/**
 * Whether a step reaches past the current page.
 *
 * Kept separate from [`isWrite`] because the two answer different questions:
 * one is "does this change something", the other "is this still inside what the
 * user granted access to".
 */
export function isCrossSite(action) {
	return CROSS_SITE_ACTIONS.has(action?.action);
}

/** Origin of a URL, or null when it isn't a URL we will touch. */
export function originOf(url) {
	let parsed;
	try {
		parsed = new URL(url);
	} catch {
		return null;
	}
	// http(s) only. `javascript:` and `data:` are script execution wearing a
	// URL; `file:` is the user's disk. The gateway rejects these too — this is
	// the copy that counts.
	if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') return null;
	return parsed.origin;
}

/**
 * Decide what has to happen before `actions` may run against `pageUrl`.
 *
 * Returns `{ ok }` when everything may proceed, or `{ ok: false, reason }` when
 * the batch must be refused outright, plus the set of domains whose writes need
 * a confirmation the user has not already given.
 */
export function evaluateBatch(actions, pageUrl, settings) {
	const conf = withDefaults(settings);
	if (!Array.isArray(actions) || actions.length === 0) {
		return { ok: false, reason: 'empty batch' };
	}

	for (const action of actions) {
		if (!KNOWN_ACTIONS.has(action?.action)) {
			return { ok: false, reason: `unsupported action: ${String(action?.action)}` };
		}
		if (action.action === 'navigate' && !originOf(action.url)) {
			return { ok: false, reason: `refusing to navigate to ${String(action.url)}` };
		}
		// Reading every open tab is not "this site", so it is not covered by a
		// per-site grant. Under the blanket grant it is.
		if (isCrossSite(action) && conf.siteAccess !== 'all_sites') {
			return {
				ok: false,
				reason: `${action.action} reports every open tab, which is not one of the sites ` +
					`you approved — switch site access to "any site" if you want that`
			};
		}
	}

	// Which origins this batch will touch: where we are now, plus anywhere it
	// navigates to. The permission check upstream is done against these, so a
	// batch that wanders onto a site the user did not grant is stopped there
	// rather than half-run.
	const origins = new Set();
	let current = originOf(pageUrl);
	for (const action of actions) {
		if (action.action === 'navigate') current = originOf(action.url);
		if (current) origins.add(current);
	}

	// A write needs somewhere to happen. On `about:blank`, a `chrome://` page or
	// a PDF viewer there is no origin to check a grant against, so acting there
	// would bypass the only gate that exists.
	if (origins.size === 0 && actions.some(isWrite)) {
		return {
			ok: false,
			reason: 'refusing to act on a page with no usable address — navigate somewhere first'
		};
	}

	return { ok: true, origins: [...origins] };
}

/**
 * The broad grant behind `all_sites`.
 *
 * `http` as well as `https`: intranet tools — a large part of why anyone wants
 * this — are routinely plain http, and a mode called "all sites" that quietly
 * excluded them would be a promise the extension does not keep.
 */
export const BROAD_ORIGINS = ['https://*/*', 'http://*/*'];

/**
 * Host permissions a batch needs.
 *
 * Note what this is **not**: a request. The permission must already be held by
 * the time a batch runs, because `chrome.permissions.request` only works inside
 * a user gesture and a message from a web page is not one. Asking here would be
 * rejected by Chrome every time. The grant is therefore taken when the user
 * arms the extension (a click on its own icon, which *is* a gesture) or adds a
 * site in settings; here we only check, and refuse with something the user can
 * act on.
 */
export function permissionsFor(origins, settings) {
	const conf = withDefaults(settings);
	// In `all_sites` the broad grant is what has to be present — returning
	// nothing would have meant "no permission needed", and then every
	// `executeScript` failed with "Cannot access contents of the url" while the
	// settings page promised the opposite.
	if (conf.siteAccess === 'all_sites') return BROAD_ORIGINS;
	return origins.map((origin) => `${origin}/*`);
}

/**
 * Whether an origin may be paired as a gateway at all.
 *
 * https everywhere, plus plain http on the loopback interface — that is where
 * the gateway is developed against, and `mise run dev` serves
 * `http://127.0.0.1:8080`. Restricting the exception to the literal string
 * "localhost" made the one setup people actually test with unpairable.
 */
export function isSafeGatewayOrigin(origin) {
    let url;
    try {
        url = new URL(origin);
    } catch {
        return false;
    }
    if (url.protocol === 'https:') return true;
    return url.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
}

/**
 * Every origin to pair when the user asks for one.
 *
 * `http://localhost:8080` and `http://127.0.0.1:8080` are the same server and
 * different origins, so pairing one and opening the other lands on "not your
 * gateway" with nothing to suggest why. Which spelling a developer ends up on
 * is not their choice either — a bookmark, a redirect, or the gateway's own
 * configured public URL decides it. So a loopback pairing covers both, in one
 * permission prompt.
 *
 * Only loopback: for a real host, two names really can be two deployments, and
 * quietly granting a second one would be a decision that is not ours.
 */
export function pairableOrigins(origin) {
	let url;
	try {
		url = new URL(origin);
	} catch {
		return [];
	}
	const siblings = { localhost: '127.0.0.1', '127.0.0.1': 'localhost' };
	const other = siblings[url.hostname];
	if (url.protocol !== 'http:' || !other) return [url.origin];
	const port = url.port ? `:${url.port}` : '';
	return [url.origin, `http://${other}${port}`];
}

/** Whether a gateway origin has been paired by the user. */
export function isPairedGateway(origin, settings) {
	return withDefaults(settings).gateways.includes(origin);
}
