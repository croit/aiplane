/**
 * Tests for the extension's own rules (run by `mise run test-extension`).
 *
 * These pin the decisions that the gateway is *not* allowed to make for us. The
 * gateway labels each step read-or-write too, and its label is advisory: it
 * lives on the same side of the trust boundary as anyone who has script on its
 * origin. So the cases below are all variations of "the request says one thing,
 * the extension must decide for itself".
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
	BROAD_ORIGINS,
	DEFAULT_SETTINGS,
	KNOWN_ACTIONS,
	evaluateBatch,
	isCrossSite,
	isSafeGatewayOrigin,
	isWrite,
	pairableOrigins,
	originOf,
	permissionsFor
} from './policy.js';

test('the default is one broad grant, not per-site prompts', () => {
	// Most people will not click through a dialog per domain, and a setting
	// nobody keeps switched on protects nobody. The trade is paid for by the
	// write confirmation, which is why that one has no global off switch.
	assert.equal(DEFAULT_SETTINGS.siteAccess, 'all_sites');
});

test('the default mode still names a permission that must be held', () => {
	// It used to return nothing, which the service worker read as "no
	// permission needed" — so it never asked for one and every executeScript
	// failed with "Cannot access contents of the url", while the settings page
	// promised a single grant asked for once.
	assert.deepEqual(permissionsFor(['https://example.com'], DEFAULT_SETTINGS), BROAD_ORIGINS);
	assert.ok(
		BROAD_ORIGINS.some((p) => p.startsWith('http://')),
		'intranet tools are routinely plain http; "all sites" must mean all sites'
	);
});

test('approved_sites asks Chrome for each origin', () => {
	assert.deepEqual(
		permissionsFor(['https://example.com'], { siteAccess: 'approved_sites' }),
		['https://example.com/*']
	);
});

test('writes are classified here, not taken on trust', () => {
	for (const action of ['navigate', 'go_back', 'click', 'drag', 'type_text', 'press_key']) {
		assert.equal(isWrite({ action }), true, `${action} must be a write`);
	}
	for (const action of [
		'read_page',
		'find',
		'scroll',
		'screenshot',
		'list_tabs',
		'wait_for',
		'set_viewport',
		// Hovering opens menus but changes nothing, and demanding a dialog for
		// it would make every menu-driven site unusable.
		'hover'
	]) {
		assert.equal(isWrite({ action }), false, `${action} must be a read`);
	}
});

test('every action the gateway can send is one the extension knows', () => {
	// The two enums are written out on both sides of the trust boundary on
	// purpose. They still have to agree about the vocabulary, or a step the
	// gateway offers the model is refused as "unsupported" at the far end.
	const fromRust = [
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
	];
	for (const action of fromRust) {
		assert.equal(KNOWN_ACTIONS.has(action), true, `${action} is missing from the extension`);
	}
	assert.equal(KNOWN_ACTIONS.size, fromRust.length, 'the extension knows an action Rust does not');
});

test('an unknown action counts as a write and is refused outright', () => {
	// A future gateway may send a step this extension does not implement. It
	// must never be run as if it were harmless, and it must not be guessed at.
	assert.equal(isWrite({ action: 'exfiltrate_cookies' }), true);
	const verdict = evaluateBatch([{ action: 'exfiltrate_cookies' }], 'https://a.example', {});
	assert.equal(verdict.ok, false);
	assert.match(verdict.reason, /unsupported/);
});

test('only http(s) navigations are accepted', () => {
	for (const url of ['javascript:alert(1)', 'data:text/html,<b>x', 'file:///etc/passwd']) {
		assert.equal(originOf(url), null, `${url} must not resolve to an origin`);
		const verdict = evaluateBatch([{ action: 'navigate', url }], 'https://a.example', {});
		assert.equal(verdict.ok, false, `${url} must be refused`);
	}
	assert.equal(originOf('https://example.com/path?q=1'), 'https://example.com');
});

test('the origins a batch will touch include where it navigates to', () => {
	// The permission check upstream runs against these, so a batch that wanders
	// onto a site the user did not grant is stopped there rather than half-run.
	const verdict = evaluateBatch(
		[
			{ action: 'navigate', url: 'https://bank.example/transfer' },
			{ action: 'type_text', ref: 'e1', text: '1000', submit: true }
		],
		'https://a.example',
		{}
	);
	assert.equal(verdict.ok, true);
	assert.ok(verdict.origins.includes('https://bank.example'), JSON.stringify(verdict.origins));
});

test('reads on a page are always fine', () => {
	const verdict = evaluateBatch(
		[{ action: 'read_page' }, { action: 'screenshot' }],
		'https://a.example',
		{}
	);
	assert.equal(verdict.ok, true);
});

test('an empty batch is refused rather than reported as a success', () => {
	assert.equal(evaluateBatch([], 'https://a.example', {}).ok, false);
	assert.equal(evaluateBatch(null, 'https://a.example', {}).ok, false);
});

test('granting a site grants working on it, with no dialog per action', () => {
	// The product decision: a prompt per click is one nobody reads by the third,
	// so a site the user granted is a site the assistant may act on.
	const verdict = evaluateBatch(
		[
			{ action: 'navigate', url: 'https://shop.example/checkout' },
			{ action: 'type_text', ref: 'e1', text: '1000', submit: true },
			{ action: 'click', ref: 'e2' }
		],
		'https://a.example',
		{}
	);
	assert.equal(verdict.ok, true);
	assert.ok(verdict.origins.includes('https://shop.example'), 'but the target is still checked');
});

test('reading every open tab is not covered by a per-site grant', () => {
	// `list_tabs` belongs to no site, so "only these sites" cannot be stretched
	// to cover it. Under the blanket grant it is fine.
	assert.equal(isCrossSite({ action: 'list_tabs' }), true);
	assert.equal(isCrossSite({ action: 'read_page' }), false);

	const limited = evaluateBatch([{ action: 'list_tabs' }], 'https://a.example', {
		siteAccess: 'approved_sites'
	});
	assert.equal(limited.ok, false);
	assert.match(limited.reason, /every open tab/);

	const blanket = evaluateBatch([{ action: 'list_tabs' }], 'https://a.example', {});
	assert.equal(blanket.ok, true);
});

test('a write on a page with no usable address is refused, not waved through', () => {
	// about:blank is exactly what the working tab starts as. With no origin
	// there is nothing to check a grant against, so acting there would bypass
	// the only gate that exists.
	for (const url of ['about:blank', 'chrome://settings', '']) {
		const verdict = evaluateBatch([{ action: 'click', ref: 'e1' }], url, {});
		assert.equal(verdict.ok, false, `${url} must be refused`);
		assert.match(verdict.reason, /no usable address/);
	}
});

test('reads on an unaddressable page are still fine', () => {
	// The refusal above is about acting, not about looking.
	const verdict = evaluateBatch([{ action: 'read_page' }], 'about:blank', {});
	assert.equal(verdict.ok, true);
});

test('the dev loopback origin can be paired, and plain http elsewhere cannot', () => {
	// `mise run dev` serves http://127.0.0.1:8080. A guard that spelled out only
	// "localhost" made the one setup people actually test with unpairable, with a
	// message that read like a policy decision rather than a typo.
	for (const ok of ['https://gw.example.com', 'http://127.0.0.1:8080', 'http://localhost:5173']) {
		assert.equal(isSafeGatewayOrigin(ok), true, `${ok} must be pairable`);
	}
	for (const bad of ['http://gw.example.com', 'http://192.168.1.10', 'ftp://x', 'not a url']) {
		assert.equal(isSafeGatewayOrigin(bad), false, `${bad} must be refused`);
	}
});

test('pairing a loopback gateway covers both spellings of it', () => {
	// localhost and 127.0.0.1 are the same server and different origins. Pairing
	// one and opening the other lands on "not your gateway", and which spelling
	// you end up on is decided by a bookmark or a redirect, not by you.
	assert.deepEqual(pairableOrigins('http://localhost:8080'), [
		'http://localhost:8080',
		'http://127.0.0.1:8080'
	]);
	assert.deepEqual(pairableOrigins('http://127.0.0.1:8080'), [
		'http://127.0.0.1:8080',
		'http://localhost:8080'
	]);
	// Default port, no colon.
	assert.deepEqual(pairableOrigins('http://localhost'), [
		'http://localhost',
		'http://127.0.0.1'
	]);
});

test('a real host is never paired together with another name', () => {
	// Two names for a public host really can be two deployments; granting the
	// second quietly would be a decision that is not the extension's to make.
	assert.deepEqual(pairableOrigins('https://gw.example.com'), ['https://gw.example.com']);
	assert.deepEqual(pairableOrigins('https://localhost:8080'), ['https://localhost:8080']);
	assert.deepEqual(pairableOrigins('nonsense'), []);
});
