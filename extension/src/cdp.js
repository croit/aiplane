/**
 * Real browser input, through the Chrome DevTools Protocol.
 *
 * The DOM-level version of this file called `element.click()` and dispatched
 * synthetic `KeyboardEvent`s. That works on a login form and fails on the web:
 * `isTrusted` is false, no pointer events are produced, drag handlers never
 * fire, and an editor that owns its own input (Docs, CodeMirror, anything on a
 * canvas) simply ignores it. `chrome.debugger` is the same interface Playwright
 * and Puppeteer drive, and the events it dispatches are indistinguishable from
 * a human's.
 *
 * The price is visible and deliberate: while attached, Chrome shows its
 * "debugging this browser" bar over the tab. That bar *is* the honest signal
 * that something else is driving, so it is left alone rather than worked
 * around. We attach on first use and detach when the extension is switched off
 * or the tab goes away.
 *
 * Coordinates are CSS pixels in the viewport, which is what
 * `Input.dispatchMouseEvent` wants and what `getBoundingClientRect` returns —
 * no device-pixel-ratio conversion, including under viewport emulation.
 */

const PROTOCOL = '1.3';

/** Tab ids we hold a debugger session on. */
const attached = new Set();

/** Chrome drops the session when the user dismisses the bar; forget it then. */
chrome.debugger.onDetach.addListener((source) => attached.delete(source.tabId));
chrome.tabs.onRemoved.addListener((tabId) => attached.delete(tabId));

export async function attach(tabId) {
	if (attached.has(tabId)) return;
	await chrome.debugger.attach({ tabId }, PROTOCOL);
	attached.add(tabId);
	// Page for navigation + screenshots, DOM/Runtime for element geometry.
	await send(tabId, 'Page.enable');
	await send(tabId, 'DOM.enable');
	await send(tabId, 'Runtime.enable');
}

export async function detach(tabId) {
	if (!attached.has(tabId)) return;
	attached.delete(tabId);
	await chrome.debugger.detach({ tabId }).catch(() => {});
}

export function send(tabId, method, params = {}) {
	return new Promise((resolve, reject) => {
		chrome.debugger.sendCommand({ tabId }, method, params, (result) => {
			if (chrome.runtime.lastError) reject(new Error(chrome.runtime.lastError.message));
			else resolve(result ?? {});
		});
	});
}

/**
 * Centre of an element, in viewport CSS pixels, after scrolling it into view.
 *
 * Scrolling first is not a nicety: a click dispatched at coordinates outside
 * the viewport lands on whatever is actually there, which is how an agent ends
 * up clicking the wrong thing and reporting success.
 */
export async function centerOf(tabId, ref) {
	const { result } = await send(tabId, 'Runtime.evaluate', {
		expression: `(() => {
			const el = document.querySelector('[data-gw-ref=${JSON.stringify(ref)}]');
			if (!el) return null;
			el.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
			const r = el.getBoundingClientRect();
			if (r.width === 0 && r.height === 0) return null;
			return JSON.stringify({ x: r.left + r.width / 2, y: r.top + r.height / 2 });
		})()`,
		returnByValue: true
	});
	if (!result?.value) {
		throw new Error(`no visible element with ref ${ref} — read the page again`);
	}
	return JSON.parse(result.value);
}

const BUTTONS = { left: 'left', right: 'right', middle: 'middle' };

/** Modifier bitmask the protocol expects: alt 1, ctrl 2, meta 4, shift 8. */
export function modifierMask(modifiers = []) {
	let mask = 0;
	for (const m of modifiers) {
		if (m === 'alt') mask |= 1;
		if (m === 'ctrl') mask |= 2;
		if (m === 'meta') mask |= 4;
		if (m === 'shift') mask |= 8;
	}
	return mask;
}

export async function moveTo(tabId, { x, y }) {
	await send(tabId, 'Input.dispatchMouseEvent', { type: 'mouseMoved', x, y, button: 'none' });
}

export async function click(tabId, { x, y }, { button = 'left', clickCount = 1 } = {}) {
	const b = BUTTONS[button] ?? 'left';
	// A real click is preceded by a move: hover styles, tooltips and menus all
	// react to the pointer arriving, and some widgets only arm on mouseover.
	await moveTo(tabId, { x, y });
	for (let i = 1; i <= clickCount; i += 1) {
		const common = { x, y, button: b, clickCount: i, buttons: b === 'left' ? 1 : 2 };
		await send(tabId, 'Input.dispatchMouseEvent', { type: 'mousePressed', ...common });
		await send(tabId, 'Input.dispatchMouseEvent', {
			type: 'mouseReleased',
			...common,
			buttons: 0
		});
	}
}

export async function drag(tabId, from, to) {
	await moveTo(tabId, from);
	await send(tabId, 'Input.dispatchMouseEvent', {
		type: 'mousePressed',
		...from,
		button: 'left',
		clickCount: 1,
		buttons: 1
	});
	// Intermediate moves, because drag implementations commonly start on the
	// first mousemove after the press and ignore a single jump to the target.
	for (const t of [0.25, 0.5, 0.75, 1]) {
		await send(tabId, 'Input.dispatchMouseEvent', {
			type: 'mouseMoved',
			x: from.x + (to.x - from.x) * t,
			y: from.y + (to.y - from.y) * t,
			button: 'left',
			buttons: 1
		});
	}
	await send(tabId, 'Input.dispatchMouseEvent', {
		type: 'mouseReleased',
		...to,
		button: 'left',
		clickCount: 1,
		buttons: 0
	});
}

export async function wheel(tabId, { x, y }, deltaY) {
	await send(tabId, 'Input.dispatchMouseEvent', {
		type: 'mouseWheel',
		x,
		y,
		deltaX: 0,
		deltaY
	});
}

/**
 * Keys the protocol needs a virtual key code for. Printable characters go
 * through `text` instead and need none.
 */
const KEYS = {
	Enter: { keyCode: 13, text: '\r' },
	Tab: { keyCode: 9, text: '\t' },
	Escape: { keyCode: 27 },
	Backspace: { keyCode: 8 },
	Delete: { keyCode: 46 },
	ArrowUp: { keyCode: 38 },
	ArrowDown: { keyCode: 40 },
	ArrowLeft: { keyCode: 37 },
	ArrowRight: { keyCode: 39 },
	Home: { keyCode: 36 },
	End: { keyCode: 35 },
	PageUp: { keyCode: 33 },
	PageDown: { keyCode: 34 }
};

export async function pressKey(tabId, key, modifiers = []) {
	const mask = modifierMask(modifiers);
	const known = KEYS[key];
	// A single printable character is sent as text; `a` with ctrl held is a
	// shortcut and must not insert an "a", so text is dropped when a
	// non-shift modifier is down.
	const printable = !known && [...key].length === 1;
	if (!known && !printable) {
		throw new Error(`unsupported key ${key} — use a single character or ${Object.keys(KEYS).join(', ')}`);
	}
	const base = {
		modifiers: mask,
		key,
		windowsVirtualKeyCode: known?.keyCode ?? key.toUpperCase().charCodeAt(0),
		nativeVirtualKeyCode: known?.keyCode ?? key.toUpperCase().charCodeAt(0)
	};
	const withText = mask & ~8 ? {} : { text: known?.text ?? (printable ? key : undefined) };
	await send(tabId, 'Input.dispatchKeyEvent', {
		type: withText.text ? 'keyDown' : 'rawKeyDown',
		...base,
		...withText
	});
	await send(tabId, 'Input.dispatchKeyEvent', { type: 'keyUp', ...base });
}

/**
 * Type into whatever is focused.
 *
 * `Input.insertText` rather than a key event per character: it produces the
 * same `beforeinput`/`input` events the browser generates for real typing (so
 * frameworks see it), handles non-ASCII without a virtual-key table, and does
 * not take a second per sentence. Keys that *mean* something — Enter to submit
 * — still go through `pressKey`.
 */
export async function insertText(tabId, text) {
	await send(tabId, 'Input.insertText', { text });
}

export async function selectAll(tabId) {
	// Ctrl/Cmd+A, then Delete. Clearing by setting `value = ''` would be a DOM
	// write again, invisible to anything watching for input.
	const meta = navigator.userAgent.includes('Mac') ? ['meta'] : ['ctrl'];
	await pressKey(tabId, 'a', meta);
	await pressKey(tabId, 'Delete');
}

/** Resize the viewport, optionally as a phone. */
export async function setViewport(tabId, { width, height, mobile }) {
	await send(tabId, 'Emulation.setDeviceMetricsOverride', {
		width,
		height,
		deviceScaleFactor: mobile ? 3 : 1,
		mobile: Boolean(mobile)
	});
	await send(tabId, 'Emulation.setTouchEmulationEnabled', {
		enabled: Boolean(mobile),
		maxTouchPoints: mobile ? 5 : 0
	});
	// The user agent has to agree with the metrics, or a server-side responsive
	// site keeps sending the desktop page and the emulation proves nothing.
	if (mobile) {
		await send(tabId, 'Emulation.setUserAgentOverride', {
			userAgent:
				'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 ' +
				'(KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1',
			platform: 'iPhone'
		});
	} else {
		await send(tabId, 'Emulation.setUserAgentOverride', { userAgent: '' }).catch(() => {});
	}
	return { width, height, mobile: Boolean(mobile) };
}

/**
 * Capture the page.
 *
 * `Page.captureScreenshot` works on a tab that is not in front, which
 * `chrome.tabs.captureVisibleTab` does not — that alone is worth the debugger
 * session, because the assistant's window sits behind the user's.
 */
export async function screenshot(tabId, { fullPage = false } = {}) {
	const params = { format: 'png', captureBeyondViewport: fullPage };
	if (fullPage) {
		const { contentSize } = await send(tabId, 'Page.getLayoutMetrics');
		if (contentSize) {
			params.clip = {
				x: 0,
				y: 0,
				width: Math.min(contentSize.width, MAX_CAPTURE_PX),
				height: Math.min(contentSize.height, MAX_CAPTURE_PX),
				scale: 1
			};
		}
	}
	const { data } = await send(tabId, 'Page.captureScreenshot', params);
	return downscale(`data:image/png;base64,${data}`);
}

/**
 * Hard ceiling on what is captured at all, before any downscaling. A page can
 * be fifty thousand pixels tall; asking the browser to rasterise that produces
 * an image nobody will look at and can exhaust the tab's memory.
 */
const MAX_CAPTURE_PX = 8_000;

/**
 * Longest edge a model actually benefits from. Vision encoders downsample to
 * roughly this anyway, so anything above it costs bytes, latency and (as this
 * project found out) an upstream that refuses the request outright:
 *
 *   upstream 500 … An exception occurred while loading IMAGE data at index 2
 *
 * That was a full-page capture going out at its native size.
 */
const MAX_EDGE = 1_400;

/** Bytes above which we switch to JPEG. A screenshot is a photo of a page, and
 * PNG is the wrong codec for one once it gets big. */
const JPEG_ABOVE_BYTES = 700_000;

async function downscale(dataUrl) {
	// `OffscreenCanvas` is available in the service worker, so this needs no
	// page and no document.
	const blob = await (await fetch(dataUrl)).blob();
	const bitmap = await createImageBitmap(blob);
	const scale = Math.min(1, MAX_EDGE / Math.max(bitmap.width, bitmap.height));
	const w = Math.max(1, Math.round(bitmap.width * scale));
	const h = Math.max(1, Math.round(bitmap.height * scale));

	if (scale === 1 && blob.size <= JPEG_ABOVE_BYTES) {
		bitmap.close();
		return dataUrl;
	}

	const canvas = new OffscreenCanvas(w, h);
	const ctx = canvas.getContext('2d');
	ctx.drawImage(bitmap, 0, 0, w, h);
	bitmap.close();

	// Try PNG first; fall back to JPEG when the result is still heavy, which is
	// what a screenshot of a photo-rich page produces.
	let out = await canvas.convertToBlob({ type: 'image/png' });
	if (out.size > JPEG_ABOVE_BYTES) {
		out = await canvas.convertToBlob({ type: 'image/jpeg', quality: 0.82 });
	}
	return await blobToDataUrl(out);
}

function blobToDataUrl(blob) {
	return new Promise((resolve, reject) => {
		const reader = new FileReader();
		reader.onload = () => resolve(String(reader.result));
		reader.onerror = () => reject(new Error('could not encode the screenshot'));
		reader.readAsDataURL(blob);
	});
}
