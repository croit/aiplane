/**
 * Screenshot capture for the feedback widget. Two strategies, both returning
 * a PNG data URL so the annotator can load either one the same way.
 *
 * 1. {@link capturePageDataUrl} — DOM re-render via snapdom. Silent, needs no
 *    permission, works everywhere including Android. snapdom inlines each
 *    element's *computed* style rather than embedding the page stylesheet,
 *    which is why it is used here and `modern-screenshot` / `html-to-image`
 *    are not: those rasterise the DOM inside an SVG `<foreignObject>` that
 *    never sees a Tailwind v4 / daisyUI layer, and produce a blank image
 *    against this UI. It still cannot reproduce `<canvas>`, WebGL or
 *    cross-origin iframes — that is what the second path is for.
 *
 * 2. {@link captureExactDataUrl} — the Screen Capture API. Grabs the
 *    compositor's real pixels (true WYSIWYG) but shows a tab-share picker and
 *    is unavailable on Android Chrome, so it is opt-in and returns `null`
 *    when the user cancels rather than failing the flow.
 *
 * Both are bounded by a timeout: a report without a screenshot is a usable
 * report, a dialog that never opens is not.
 */
import { snapdom } from '@zumer/snapdom';

/**
 * Hard cap on the DOM capture. A page with a very long list can otherwise
 * spend many seconds rasterising, and the FAB would sit spinning.
 */
export const CAPTURE_TIMEOUT_MS = 10000;

/**
 * Supersampling factor. Matches the device's real pixel density so a phone
 * with a fractional DPR captures at full resolution instead of a fixed 2×
 * that under-samples it, rounded up so fractional DPRs gain rather than lose
 * pixels, and capped at 3 so a 4K/high-DPR surface does not blow up the
 * canvas and trip {@link CAPTURE_TIMEOUT_MS}.
 */
export function captureScale(dpr: number = typeof window === 'undefined' ? 1 : window.devicePixelRatio): number {
	if (!Number.isFinite(dpr) || dpr <= 0) return 1;
	return Math.min(Math.ceil(dpr), 3);
}

/**
 * The widget's own chrome must never appear in its own screenshot. Marked
 * with data attributes rather than ids so the Svelte components can move
 * without breaking this.
 */
const SHOT_EXCLUDE = ['[data-feedback-fab]', '[data-feedback-dialog]', '[data-feedback-toast]'];

/** Resolve the page background so transparent regions don't render black. */
function backgroundColor(): string {
	const pick = (el: Element | null): string => {
		if (!el) return '';
		const c = getComputedStyle(el).backgroundColor;
		return c && c !== 'rgba(0, 0, 0, 0)' && c !== 'transparent' ? c : '';
	};
	return pick(document.documentElement) || pick(document.body) || '#ffffff';
}

/**
 * Native form controls capture blank: a daisyUI toggle / checkbox / radio
 * draws its knob with a `::before` pseudo-element and a box-shadow on an
 * `appearance: none` input, and snapdom reads neither. Before capturing,
 * stamp a stand-in over each one that COPIES the control's real computed
 * track style and its `::before` knob, so the result is styled identically to
 * the live UI rather than approximated. Returns the undo.
 */
function stampControlStates(): () => void {
	const made: HTMLElement[] = [];
	const hidden: Array<{ el: HTMLElement; prev: string }> = [];

	// A div positioned exactly over `el`, carrying the control's own track
	// look. The real control is also hidden (`visibility`, so layout is kept)
	// — otherwise its still-rendered track sits a sub-pixel off ours and the
	// capture shows a doubled border.
	const trackOverlay = (el: HTMLElement, cs: CSSStyleDeclaration): HTMLDivElement => {
		const r = el.getBoundingClientRect();
		const d = document.createElement('div');
		d.style.cssText =
			`position:absolute;left:${r.left + window.scrollX}px;top:${r.top + window.scrollY}px;` +
			`width:${r.width}px;height:${r.height}px;box-sizing:border-box;` +
			`background:${cs.backgroundColor};border:${cs.border};border-radius:${cs.borderRadius};` +
			'pointer-events:none;z-index:2147483646;';
		document.body.appendChild(d);
		made.push(d);
		hidden.push({ el, prev: el.style.visibility });
		el.style.visibility = 'hidden';
		return d;
	};

	const controls = document.querySelectorAll<HTMLInputElement>(
		'input[type=checkbox], input[type=radio]'
	);
	for (const el of controls) {
		const r = el.getBoundingClientRect();
		if (r.width === 0 || r.height === 0) continue;
		const checked = el.checked;
		const cs = getComputedStyle(el);
		const before = getComputedStyle(el, '::before');
		const pad = parseFloat(cs.paddingLeft) || 3;
		const border = parseFloat(cs.borderLeftWidth) || 0;

		if (el.classList.contains('toggle')) {
			// Track + knob: copy the knob's exact size, shape (a rounded square,
			// NOT a full circle) and colour, placed `padding` from the active side.
			const box = trackOverlay(el, cs);
			const kw = parseFloat(before.width) || Math.max(4, r.height - 2 * pad);
			const kh = parseFloat(before.height) || kw;
			const innerW = r.width - 2 * border;
			const innerH = r.height - 2 * border;
			const left = checked ? innerW - kw - pad : pad;
			const top = (innerH - kh) / 2;
			const knob = document.createElement('div');
			knob.style.cssText =
				`position:absolute;left:${left}px;top:${top}px;` +
				`width:${kw}px;height:${kh}px;border-radius:${before.borderRadius || '9999px'};` +
				`background:${before.backgroundColor};`;
			box.appendChild(knob);
		} else if (el.type === 'radio') {
			const box = trackOverlay(el, cs);
			if (checked) {
				const dw = parseFloat(before.width) || Math.max(4, Math.round(r.width * 0.45));
				const dh = parseFloat(before.height) || dw;
				const dot = document.createElement('div');
				dot.style.cssText =
					'position:absolute;left:50%;top:50%;transform:translate(-50%,-50%);' +
					`width:${dw}px;height:${dh}px;border-radius:${before.borderRadius || '9999px'};` +
					`background:${before.backgroundColor};`;
				box.appendChild(dot);
			}
		} else if (checked) {
			const box = trackOverlay(el, cs);
			box.style.color = cs.color;
			box.style.display = 'flex';
			box.style.alignItems = 'center';
			box.style.justifyContent = 'center';
			box.style.fontSize = `${Math.max(8, r.height - 4)}px`;
			box.style.lineHeight = '1';
			box.textContent = '✓';
		}
	}

	return () => {
		for (const d of made) d.remove();
		for (const { el, prev } of hidden) el.style.visibility = prev;
	};
}

/**
 * Capture the page as the user currently sees it. Call it while the dialog is
 * closed, so the shot is "the UI at the moment the button was pressed".
 * Returns `null` on any failure — the dialog then opens without a screenshot.
 */
export async function capturePageDataUrl(): Promise<string | null> {
	const restore = stampControlStates();
	try {
		const result = await Promise.race([
			snapdom(document.body, {
				dpr: captureScale(),
				backgroundColor: backgroundColor(),
				exclude: SHOT_EXCLUDE,
				excludeMode: 'remove'
			}),
			new Promise<never>((_, reject) =>
				window.setTimeout(() => reject(new Error('timeout')), CAPTURE_TIMEOUT_MS)
			)
		]);
		const canvas = await result.toCanvas();
		if (!canvas.width || !canvas.height) return null;
		return canvas.toDataURL('image/png');
	} catch {
		return null;
	} finally {
		restore();
	}
}

/**
 * Whether the Screen Capture API is available. Android Chrome exposes the
 * method but always rejects it; presence still counts as "supported" and the
 * rejection falls back, because there is no reliable pre-flight for that.
 */
export function isExactCaptureSupported(): boolean {
	return (
		typeof navigator !== 'undefined' && typeof navigator.mediaDevices?.getDisplayMedia === 'function'
	);
}

// `preferCurrentTab` is a Chrome extension to the standard options that nudges
// the picker to default to the current tab. Not in lib.dom.d.ts.
interface DisplayMediaWithCurrentTab extends DisplayMediaStreamOptions {
	preferCurrentTab?: boolean;
}

/**
 * Capture the exact on-screen pixels via `getDisplayMedia`. MUST be called
 * from a user-gesture handler (it consumes transient activation). Returns
 * `null` when unsupported or when the user cancels the picker, so the caller
 * keeps whatever screenshot it already had.
 */
export async function captureExactDataUrl(): Promise<string | null> {
	if (!isExactCaptureSupported()) return null;

	let stream: MediaStream;
	try {
		const constraints: DisplayMediaWithCurrentTab = {
			preferCurrentTab: true,
			video: { displaySurface: 'browser' },
			audio: false
		};
		stream = await navigator.mediaDevices.getDisplayMedia(constraints);
	} catch {
		// NotAllowedError (cancelled picker) or an unsupported surface.
		return null;
	}

	try {
		const [track] = stream.getVideoTracks();
		const video = document.createElement('video');
		video.srcObject = stream;
		video.muted = true;
		video.playsInline = true;
		await video.play();
		await waitForVideoFrame(video);

		const settings = track?.getSettings();
		const width = settings?.width ?? video.videoWidth;
		const height = settings?.height ?? video.videoHeight;
		if (!width || !height) return null;

		const canvas = document.createElement('canvas');
		canvas.width = width;
		canvas.height = height;
		const ctx = canvas.getContext('2d');
		if (!ctx) return null;
		ctx.drawImage(video, 0, 0, width, height);
		return canvas.toDataURL('image/png');
	} finally {
		for (const t of stream.getTracks()) t.stop();
	}
}

/** Resolves once the video has painted a frame, so the canvas isn't blank. */
function waitForVideoFrame(video: HTMLVideoElement): Promise<void> {
	return new Promise<void>((resolve) => {
		const rvfc = (
			video as HTMLVideoElement & {
				requestVideoFrameCallback?: (cb: () => void) => number;
			}
		).requestVideoFrameCallback;
		if (typeof rvfc === 'function') {
			rvfc.call(video, () => resolve());
		} else {
			requestAnimationFrame(() => resolve());
		}
	});
}

/** `data:image/png;base64,AAA…` → `AAA…`, which is what the API wants. */
export function stripDataUrlPrefix(dataUrl: string): string {
	const comma = dataUrl.indexOf(',');
	return comma === -1 ? dataUrl : dataUrl.slice(comma + 1);
}
