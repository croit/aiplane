#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH
//
// Capture the three 1280×800 screenshots the Chrome Web Store listing needs.
//
//   mise run dev-ui                                       # another terminal
//   mise run extension-screenshots -- --cookie "id=<the cookie dev-ui printed>"
//
// The store shows these at the top of the listing and they are the only part a
// reviewer — and everyone else — looks at before reading a word, so they are
// generated rather than collected: a listing whose pictures still say
// "LLM Gateway" is what happens when three PNGs live only in somebody's
// Downloads folder.
//
// **Every pixel here is a real capture.** Shot 1 is the SPA rendering a real
// conversation out of the dev-UI harness's database. Shots 2 and 3 are the
// extension's own popup and options pages, rendered by the extension loaded
// unpacked into this browser, reading the state the extension itself wrote.
// Nothing is drawn or mocked up; the compositing below only places real
// captures on a 1280×800 canvas, which is a size the store insists on and no
// single window produces.
//
// Two things cannot be driven by automation, and are stood in for rather than
// faked:
//
//   * **The host permission.** `chrome.permissions.request` puts up browser
//     chrome, not a page, so Playwright cannot accept it. The capture arms the
//     extension through the same `arm` message the popup's button sends, which
//     is the half that is ours; Chrome's own dialog is the half that is not.
//   * **Which tab is in front.** A real popup hangs off the toolbar, so the
//     tab it inspects is the one behind it. Opened as a tab — the only way
//     there is to screenshot it — the tab it inspects is itself, and it would
//     read "not your AIplane". `chrome.tabs.query` is therefore answered with
//     the AIplane the extension is paired with and armed for, which is the tab
//     a user would have in front of them.
//
// Everything else — the pairing, the armed state, the activity log, the site
// access mode — is written through the extension's own storage and read back
// by its own code.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const args = Object.fromEntries(
	process.argv.slice(2).reduce((acc, a, i, arr) => {
		if (a.startsWith('--')) acc.push([a.slice(2), arr[i + 1]?.startsWith('--') ? 'true' : arr[i + 1]]);
		return acc;
	}, [])
);

const ROOT = path.resolve(import.meta.dirname, '../..');
const EXTENSION = path.join(ROOT, 'extension');
const OUT_DIR = path.join(EXTENSION, 'store');
const GATEWAY = args.gateway ?? 'http://localhost:8080';
const COOKIE = (args.cookie ?? '').replace(/^id=/, '');
const KEEP = args.keep === 'true';

// The store's screenshot size. 640×400 is also allowed and nobody uses it.
const W = 1280;
const H = 800;
// Everything is captured at twice that and resampled down — see `render`.
const SCALE = 2;

// What the popup and the conversation both describe. A documentation domain
// (RFC 2606) rather than a real supplier, and an AIplane on a hostname that
// reads like the self-hosted install it is.
const AIPLANE_ORIGIN = 'https://aiplane.example.com';

// ---------------------------------------------------------------------------
// The mise-managed Playwright, and the Chromium it has cached. Same discovery
// as .claude/skills/take-screenshots/screenshot.mjs — the bundled Playwright's
// expected browser revision routinely differs from what is on disk, and
// launching the cached binary by path sidesteps the "run playwright install"
// dead end.
// ---------------------------------------------------------------------------
function discoverPlaywright() {
	if (process.env.PLAYWRIGHT_DIR) return process.env.PLAYWRIGHT_DIR;
	const base = path.join(os.homedir(), '.local/share/mise/installs/npm-playwright-cli');
	const versions = fs.existsSync(base)
		? fs.readdirSync(base).filter((v) => /^\d+\.\d+\.\d+$/.test(v)).sort().reverse()
		: [];
	for (const v of versions) {
		const dir = path.join(base, v, 'lib/node_modules/@playwright/cli/node_modules/playwright');
		if (fs.existsSync(path.join(dir, 'index.mjs'))) return dir;
	}
	throw new Error(
		'could not find the mise Playwright lib — run `mise install`, or set PLAYWRIGHT_DIR to the playwright package directory'
	);
}

function discoverChromium() {
	if (process.env.CHROME_EXE) return process.env.CHROME_EXE;
	const cache = path.join(os.homedir(), 'Library/Caches/ms-playwright');
	const revs = fs.existsSync(cache)
		? fs
				.readdirSync(cache)
				.filter((d) => /^chromium-\d+$/.test(d))
				.sort((a, b) => Number(b.split('-')[1]) - Number(a.split('-')[1]))
		: [];
	for (const rev of revs) {
		for (const arch of ['chrome-mac-arm64', 'chrome-mac-x64', 'chrome-mac', 'chrome-linux']) {
			for (const exe of ['Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing', 'chrome']) {
				const candidate = path.join(cache, rev, arch, exe);
				if (fs.existsSync(candidate)) return candidate;
			}
		}
	}
	return null;
}

// ---------------------------------------------------------------------------
// The state a real session leaves behind, written through the extension's own
// storage so its own code renders it.
// ---------------------------------------------------------------------------

// Fixed, not `Date.now()`: the popup prints the clock time of every step, and
// a wall-clock stamp would rewrite all three PNGs on every run for no change.
const AT = (seconds) => `2026-09-19T09:41:${String(seconds).padStart(2, '0')}.000Z`;

// Exactly the entries `note()` in background.js writes: `armed` when the user
// switches it on, then one `ran` per action in the batch, newest first.
const ACTIVITY = [
	{ at: AT(29), event: 'ran', action: 'read_page' },
	{ at: AT(27), event: 'ran', action: 'wait_for' },
	{ at: AT(24), event: 'ran', action: 'navigate' },
	{ at: AT(11), event: 'armed', origin: AIPLANE_ORIGIN }
];

async function main() {
	const playwright = discoverPlaywright();
	const chromeExe = discoverChromium();
	const { chromium } = await import(`${playwright}/index.mjs`);

	fs.mkdirSync(OUT_DIR, { recursive: true });
	const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'aiplane-store-shots-'));

	const context = await chromium.launchPersistentContext(profile, {
		...(chromeExe ? { executablePath: chromeExe } : {}),
		// MV3 service workers do not start in headless Chromium, and without one
		// there is no extension to screenshot.
		headless: false,
		args: [`--disable-extensions-except=${EXTENSION}`, `--load-extension=${EXTENSION}`],
		viewport: { width: W, height: H },
		deviceScaleFactor: SCALE,
		colorScheme: 'dark',
		locale: 'en-US'
	});

	try {
		if (COOKIE) {
			await context.addCookies([{ name: 'id', value: COOKIE, url: new URL(GATEWAY).origin }]);
		}

		const worker =
			context.serviceWorkers()[0] ?? (await context.waitForEvent('serviceworker', { timeout: 15000 }));
		const extensionId = new URL(worker.url()).host;
		console.log(`extension ${extensionId}`);

		const conversation = await shotConversation(context);
		const popup = await shotPopup(context, extensionId);
		await shotSettings(context, extensionId);
		await composePopupOverPage(context, conversation, popup);
	} finally {
		await context.close();
		if (!KEEP) fs.rmSync(profile, { recursive: true, force: true });
	}
}

/**
 * Shot 1 — the product doing the thing: a conversation in which the assistant
 * worked in the user's browser and answered from what it found.
 *
 * Returned as well as saved, because it is also the page the popup hangs over
 * in shot 2 — which is where a real popup is, and saves inventing a backdrop.
 */
async function shotConversation(context) {
	const sessionId = await findSeededSession(context);
	const page = await context.newPage();
	await page.goto(`${GATEWAY}/chat/${sessionId}`, { waitUntil: 'networkidle' });
	await page.waitForSelector('text=Order 48217', { timeout: 15000 }).catch(() => {
		console.warn('WARN: the seeded browser_control conversation did not render; capturing anyway');
	});
	// The SPA opens a conversation at its foot, which cuts off the question the
	// whole shot is about. The tool call stays collapsed: expanded, its JSON is
	// four screens of it and pushes the answer out of frame.
	await page.evaluate(() => {
		for (const el of document.querySelectorAll('*')) {
			if (el.scrollHeight > el.clientHeight + 40 && getComputedStyle(el).overflowY !== 'visible') {
				el.scrollTop = 0;
			}
		}
	});
	await page.waitForTimeout(600);
	const shot = await page.screenshot();
	await save(context, shot, '01-conversation.png');
	await page.close();
	return shot;
}

/** The dev-UI harness seeds it; ask the API rather than hardcode a uuid. */
async function findSeededSession(context) {
	const response = await context.request.get(`${GATEWAY}/api/v0/chat/sessions`);
	if (!response.ok()) {
		throw new Error(
			`GET ${GATEWAY}/api/v0/chat/sessions → ${response.status()} — is the dev-UI gateway ` +
				`up, and is --cookie the one it printed?`
		);
	}
	const list = (await response.json()).sessions ?? [];
	const match = list.find((s) => (s.title ?? '').includes('48217'));
	if (!match) {
		throw new Error(
			`no seeded browser_control conversation at ${GATEWAY} — is this the dev-UI harness ` +
				`(mise run dev-ui) and is the cookie the one it printed? Found: ` +
				list.map((s) => s.title).join(', ')
		);
	}
	return match.id;
}

/**
 * Shot 2's foreground — the popup as it looks while the extension is on, with
 * the steps it carried out listed underneath.
 */
async function shotPopup(context, extensionId) {
	// Pair and arm through the extension's own surfaces first, from one of its
	// own pages — `chrome.storage` and `chrome.runtime` are only reachable from
	// an extension context.
	const seeder = await context.newPage();
	await seeder.goto(`chrome-extension://${extensionId}/src/options.html`);
	await seeder.evaluate(
		async ([origin, activity]) => {
			await chrome.storage.local.set({ settings: { gateways: [origin], siteAccess: 'all_sites' } });
			await chrome.runtime.sendMessage({ type: 'arm', origin });
			await chrome.storage.session.set({ activity });
		},
		[AIPLANE_ORIGIN, ACTIVITY]
	);
	await seeder.close();

	const page = await context.newPage();
	// See the header: opened as a tab, the popup would inspect itself. Point it
	// at the AIplane the user would have in front of them.
	await page.addInitScript((origin) => {
		const real = chrome.tabs.query.bind(chrome.tabs);
		chrome.tabs.query = (info, ...rest) =>
			info?.active === true
				? Promise.resolve([{ id: 1, url: `${origin}/chat`, active: true }])
				: real(info, ...rest);
	}, AIPLANE_ORIGIN);
	// The popup's own width, and a viewport short enough that `fullPage` below
	// measures the content rather than padding the shot out to the window.
	await page.setViewportSize({ width: 332, height: 160 });
	await page.goto(`chrome-extension://${extensionId}/src/popup.html`);
	await page.waitForSelector('#toggle:not([hidden])', { timeout: 10000 });
	await page.waitForSelector('#activity li', { timeout: 10000 });
	await page.waitForTimeout(400);
	const shot = await page.screenshot({ fullPage: true });
	await page.close();
	return shot;
}

/** Shot 3 — the settings page, where the AIplane is paired and site access chosen. */
async function shotSettings(context, extensionId) {
	const page = await context.newPage();
	await page.goto(`chrome-extension://${extensionId}/src/options.html`);
	await page.waitForSelector('#aiplanes li', { timeout: 10000 });
	await page.waitForTimeout(400);
	await save(context, await page.screenshot(), '03-settings.png');
	await page.close();
}

/**
 * Shot 2 — the popup over the page it is switched on for, which is where a
 * user sees it. Both layers are captures taken above; this only lays them out
 * at the size the store wants.
 */
async function composePopupOverPage(context, backdrop, popup) {
	const html = `
		<style>
			.backdrop { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover;
				filter: brightness(0.38) saturate(0.8); }
			.popup { position: absolute; top: 36px; right: 40px; width: 440px; border-radius: 14px;
				box-shadow: 0 30px 80px rgba(0,0,0,.7), 0 0 0 1px rgba(255,255,255,.1); }
			.band { position: absolute; inset: auto 0 0 0; height: 300px;
				background: linear-gradient(to top, rgba(12,12,14,.96) 30%, rgba(12,12,14,0)); }
			.caption { position: absolute; left: 56px; bottom: 56px; max-width: 700px; color: #fff;
				font: 600 32px/1.22 system-ui, sans-serif; letter-spacing: -0.015em; }
			.caption small { display: block; margin-top: 14px; font: 400 19px/1.5 system-ui, sans-serif;
				color: #b4b8c0; }
		</style>
		<img class="backdrop" src="{{backdrop}}" />
		<div class="band"></div>
		<img class="popup" src="{{popup}}" />
		<div class="caption">On only while you say so
			<small>Switched on from the toolbar, never by a page — and every step it takes is listed.</small>
		</div>`;
	await save(
		context,
		await render(context, html, { backdrop, popup }),
		'02-switched-on.png'
	);
}

/**
 * Lay out images on a 1280×800 canvas and capture the result.
 *
 * In the browser rather than an image library on purpose: it keeps the whole
 * capture to one dependency, and the resampling of the 2× captures is done by
 * the engine that rendered them.
 *
 * The canvas is laid out at 1280×800 and scaled by 1/SCALE into a viewport
 * that is correspondingly smaller, so a context whose deviceScaleFactor is
 * SCALE writes a file at exactly 1280×800. The transform also makes the canvas
 * the containing block, which is why the layouts above position against it.
 */
async function render(context, html, images) {
	const page = await context.newPage();
	await page.setViewportSize({ width: W / SCALE, height: H / SCALE });
	let body = html;
	for (const [name, buffer] of Object.entries(images)) {
		body = body.replace(`{{${name}}}`, `data:image/png;base64,${buffer.toString('base64')}`);
	}
	await page.setContent(
		`<style>html, body { margin: 0; background: #1b1b1d; overflow: hidden; }
		#canvas { position: relative; width: ${W}px; height: ${H}px; overflow: hidden;
			transform: scale(${1 / SCALE}); transform-origin: 0 0; }</style>
		<div id="canvas">${body}</div>`
	);
	await page.waitForFunction(() => [...document.images].every((i) => i.complete && i.naturalWidth));
	const shot = await page.screenshot();
	await page.close();
	return shot;
}

/**
 * Write a capture at exactly 1280×800.
 *
 * The captures are taken at deviceScaleFactor 2 so the text is rendered at
 * twice the size and then resampled, which is visibly cleaner than rendering
 * 1280 wide — and the store rejects anything that is not 1280×800 or 640×400.
 */
async function save(context, shot, name) {
	const out = path.join(OUT_DIR, name);
	const html = `
		<style>img { position: absolute; inset: 0; width: 100%; height: 100%;
			object-fit: cover; object-position: top; }</style>
		<img src="{{shot}}" />`;
	const png = await render(context, html, { shot });
	fs.writeFileSync(out, png);
	// The one property the store checks before a human ever sees the listing.
	// Read off the PNG header rather than shelled out to a tool that exists on
	// one of the two platforms this runs on.
	const [width, height] = [png.readUInt32BE(16), png.readUInt32BE(20)];
	if (width !== W || height !== H) {
		throw new Error(`${name} came out ${width}×${height}; the store takes ${W}×${H} only`);
	}
	console.log(`${path.relative(ROOT, out)}  ${width}×${height}`);
}

await main();
