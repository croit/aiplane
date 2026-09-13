/**
 * The feedback widget's state and side effects.
 *
 * Restores the full pre-SPA widget (and its `croit.erp` ancestor) on the
 * SvelteKit stack: an annotated viewport screenshot, voice-to-fields
 * dictation, pasted image attachments, and the browser diagnostics the report
 * is actually useful for. The component in
 * `$lib/components/feedback/FeedbackDialog.svelte` renders this; everything
 * that is not markup lives here.
 *
 * Two orderings matter and are not incidental:
 *
 *   - **Capture happens before the dialog opens.** The screenshot has to show
 *     the UI the reporter is complaining about, not the dialog covering it.
 *     Recapture therefore closes the dialog, waits a frame, grabs, reopens.
 *   - **Capture only ever runs from an explicit user action.** An
 *     `open && !screenshot` effect looks tidy and is a trap: when capture
 *     keeps failing it retries forever, each retry closing and reopening the
 *     dialog, until the page locks up. (That is `croit.erp` #1355, and it is
 *     why `openDialog()` takes the capture into its own hands.)
 *
 * Endpoints are `/api/v0/feedback{,/config,/extract}` and, for dictation, the
 * existing `/api/v0/transcriptions`.
 */
import { base } from '$app/paths';
import { getConsoleLogs, getNetworkLogs } from './feedback-capture';
import {
	capturePageDataUrl,
	captureExactDataUrl,
	isExactCaptureSupported,
	stripDataUrlPrefix
} from './feedback-screenshot';
import { t } from './i18n.svelte';
import { me } from './session.svelte';
import {
	recordingErrorMessage,
	recordingUnavailableReason,
	startRecording,
	type VoiceRecorder
} from './voice-recorder';

/** Largest single pasted image, in bytes. Mirrors the ERP widget's limit. */
export const MAX_ATTACHMENT_BYTES = 5 * 1024 * 1024;

export type ShotStatus = 'none' | 'capturing' | 'attached' | 'failed';
export type VoicePhase = 'idle' | 'recording' | 'working';

export interface FeedbackAttachment {
	/** Object URL for the thumbnail; revoked when the image is removed. */
	url: string;
	/** Raw base64 PNG/JPEG bytes, ready for the API. */
	base64: string;
}

export const feedback = $state({
	// Config, from GET /feedback/config.
	enabled: false,
	voiceEnabled: false,
	voiceModel: '',
	provider: 'github',
	maxAttachments: 5,

	// Dialog state.
	open: false,
	confirming: false,
	busy: false,
	/** Set once the issue is filed; the dialog shows a thank-you panel. */
	filed: null as { number: number; url: string } | null,
	error: null as string | null,
	notice: null as string | null,

	// Form.
	title: '',
	description: '',
	business: '',
	acceptance: '',
	priority: 'medium',

	// Screenshot. `shotDataUrl` is what the annotator loads; the annotated
	// export is read back from the canvas at submit time.
	shotDataUrl: null as string | null,
	shotStatus: 'none' as ShotStatus,
	exactSupported: false,
	capturingExact: false,
	/**
	 * Annotation mode: the form steps aside and the capture takes the whole
	 * dialog. Drawing on a thumbnail wedged between two textareas is not
	 * drawing — you need room and you need to be able to move around a
	 * zoomed-in view, which is the whole point of this being a mode.
	 */
	annotating: false,

	// Pasted / dropped images.
	attachments: [] as FeedbackAttachment[],

	// Diagnostics consent — default on, opt out per submission.
	includeBrowserLog: true,
	includeChatLog: true,

	// Voice intake.
	voicePhase: 'idle' as VoicePhase
});

/**
 * Reads the annotated canvas. Set by the dialog component while the annotator
 * is mounted; `null` when there is no screenshot to export.
 */
let readAnnotatedShot: (() => string | null) | null = null;

export function setShotExporter(fn: (() => string | null) | null): void {
	readAnnotatedShot = fn;
}

export async function loadConfig(): Promise<void> {
	try {
		const res = await fetch('/api/v0/feedback/config', { headers: { accept: 'application/json' } });
		if (!res.ok) return;
		const cfg = (await res.json()) as {
			enabled?: boolean;
			voice_enabled?: boolean;
			voice_model?: string | null;
			provider?: string;
			max_attachments?: number;
		};
		feedback.enabled = cfg.enabled === true;
		feedback.voiceEnabled = cfg.voice_enabled === true;
		feedback.voiceModel = cfg.voice_model ?? '';
		feedback.provider = cfg.provider ?? 'github';
		if (typeof cfg.max_attachments === 'number' && cfg.max_attachments > 0) {
			feedback.maxAttachments = cfg.max_attachments;
		}
		feedback.exactSupported = isExactCaptureSupported();
	} catch {
		/* widget stays hidden — unconfigured, or the gateway is unreachable */
	}
}

/** One animation frame, so a DOM change is painted before we capture. */
const nextFrame = (): Promise<void> =>
	new Promise((resolve) => requestAnimationFrame(() => resolve()));

/**
 * Open the dialog, capturing the page first.
 *
 * The await is deliberate: the FAB shows its busy state, the page is grabbed
 * as the reporter sees it, and only then does the dialog appear over it.
 */
export async function openDialog(): Promise<void> {
	if (!feedback.enabled || feedback.shotStatus === 'capturing') return;
	feedback.error = null;
	feedback.notice = null;
	feedback.filed = null;
	feedback.shotStatus = 'capturing';
	feedback.shotDataUrl = null;
	const dataUrl = await capturePageDataUrl();
	applyShot(dataUrl);
	feedback.open = true;
}

/** Re-grab the page with the dialog out of the way, then come back. */
export async function recapture(): Promise<void> {
	if (feedback.shotStatus === 'capturing') return;
	feedback.open = false;
	feedback.shotStatus = 'capturing';
	await nextFrame();
	const dataUrl = await capturePageDataUrl();
	applyShot(dataUrl);
	feedback.open = true;
}

/**
 * The opt-in pixel-perfect path. Must run straight from the click handler —
 * `getDisplayMedia` consumes transient activation, so anything awaited before
 * it would make the browser refuse the picker.
 */
export async function captureExact(): Promise<void> {
	if (feedback.capturingExact) return;
	feedback.capturingExact = true;
	feedback.open = false;
	try {
		const dataUrl = await captureExactDataUrl();
		if (dataUrl) {
			applyShot(dataUrl);
		} else {
			// Cancelled picker or an unsupported surface: keep what we have.
			feedback.notice = t('feedback-shot-exact-cancelled');
		}
	} catch {
		feedback.error = t('feedback-shot-capture-failed');
	} finally {
		feedback.open = true;
		feedback.capturingExact = false;
	}
}

function applyShot(dataUrl: string | null): void {
	feedback.shotDataUrl = dataUrl;
	feedback.shotStatus = dataUrl ? 'attached' : 'failed';
	// A failed recapture must not strand the dialog in a mode with nothing in
	// it to annotate.
	if (!dataUrl) feedback.annotating = false;
}

export function removeShot(): void {
	feedback.shotDataUrl = null;
	feedback.shotStatus = 'none';
	feedback.annotating = false;
}

/** Enter/leave the full-size annotation view. */
export function setAnnotating(on: boolean): void {
	feedback.annotating = on && Boolean(feedback.shotDataUrl);
}

export function closeDialog(): void {
	feedback.open = false;
	feedback.confirming = false;
}

/** Drop everything the dialog was holding, including the object URLs. */
export function resetDialog(): void {
	feedback.title = '';
	feedback.description = '';
	feedback.business = '';
	feedback.acceptance = '';
	feedback.priority = 'medium';
	feedback.error = null;
	feedback.notice = null;
	feedback.filed = null;
	feedback.confirming = false;
	feedback.annotating = false;
	removeShot();
	clearAttachments();
	feedback.includeBrowserLog = true;
	feedback.includeChatLog = true;
}

// ---------------------------------------------------------------------------
// Attachments

/**
 * Accept one image. Rejections are reported as a notice rather than thrown:
 * dropping six screenshots on a five-slot form is a normal thing to do and
 * should say so, not fail silently.
 */
export async function addAttachment(file: Blob): Promise<void> {
	if (!file.type.startsWith('image/')) {
		feedback.notice = t('feedback-attachments-invalid');
		return;
	}
	if (feedback.attachments.length >= feedback.maxAttachments) {
		feedback.notice = t('feedback-attachments-too-many', { max: feedback.maxAttachments });
		return;
	}
	if (file.size > MAX_ATTACHMENT_BYTES) {
		feedback.notice = t('feedback-attachments-too-large', {
			max: Math.round(MAX_ATTACHMENT_BYTES / (1024 * 1024))
		});
		return;
	}
	const base64 = await blobToBase64(file);
	feedback.attachments.push({ url: URL.createObjectURL(file), base64 });
}

export function removeAttachment(index: number): void {
	const [removed] = feedback.attachments.splice(index, 1);
	if (removed) URL.revokeObjectURL(removed.url);
}

function clearAttachments(): void {
	for (const a of feedback.attachments) URL.revokeObjectURL(a.url);
	feedback.attachments = [];
}

/**
 * FileReader rather than `btoa(String.fromCharCode(...))`: a multi-megabyte
 * image blows the argument limit and throws on the spread.
 */
function blobToBase64(blob: Blob): Promise<string> {
	return new Promise((resolve, reject) => {
		const reader = new FileReader();
		reader.onload = () => {
			const result = typeof reader.result === 'string' ? reader.result : '';
			resolve(stripDataUrlPrefix(result));
		};
		reader.onerror = () => reject(reader.error ?? new Error('read failed'));
		reader.readAsDataURL(blob);
	});
}

// ---------------------------------------------------------------------------
// Voice → fields

let recorder: VoiceRecorder | null = null;

/**
 * Tap to talk, tap to fill. One recording becomes all five form fields: the
 * audio goes to `/api/v0/transcriptions` (the same VAD + Whisper path the
 * chat composer uses) and the transcript to `/api/v0/feedback/extract`, which
 * runs it through the operator-configured chat model. The user never picks a
 * model — neither one is theirs to choose.
 */
export async function toggleVoice(): Promise<void> {
	feedback.error = null;
	feedback.notice = null;

	if (recorder) {
		const current = recorder;
		recorder = null;
		feedback.voicePhase = 'working';
		let wav: Blob;
		try {
			wav = await current.stop();
		} catch (err) {
			feedback.voicePhase = 'idle';
			feedback.error = String(err);
			return;
		}
		if (!wav.size) {
			feedback.voicePhase = 'idle';
			feedback.notice = t('feedback-voice-no-speech');
			return;
		}
		try {
			await extractFields(wav);
		} finally {
			feedback.voicePhase = 'idle';
		}
		return;
	}

	const unavailable = recordingUnavailableReason();
	if (unavailable) {
		feedback.error = unavailable;
		return;
	}
	try {
		recorder = await startRecording(`${base}/pcm-recorder.js`);
		feedback.voicePhase = 'recording';
	} catch (err) {
		feedback.error = recordingErrorMessage(err);
	}
}

async function extractFields(wav: Blob): Promise<void> {
	try {
		const form = new FormData();
		form.append('model', feedback.voiceModel);
		form.append('file', wav, 'feedback.wav');
		const tResp = await fetch('/api/v0/transcriptions', { method: 'POST', body: form });
		if (!tResp.ok) {
			feedback.error = await errorMessage(tResp);
			return;
		}
		const transcript = ((await tResp.json()) as { text?: string }).text?.trim() ?? '';
		if (!transcript) {
			feedback.notice = t('feedback-voice-no-speech');
			return;
		}

		const locale = document.documentElement.lang || navigator.language || '';
		const xResp = await fetch('/api/v0/feedback/extract', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ transcript, locale })
		});
		if (!xResp.ok) {
			feedback.error = await errorMessage(xResp);
			return;
		}
		const f = (await xResp.json()) as {
			title?: string;
			description?: string;
			business_value?: string;
			acceptance_criteria?: string;
			priority?: string;
		};
		// Only overwrite what the model actually produced — an empty field
		// means "not derivable", not "clear what the user already typed".
		if (f.title) feedback.title = f.title;
		if (f.description) feedback.description = f.description;
		if (f.business_value) feedback.business = f.business_value;
		if (f.acceptance_criteria) feedback.acceptance = f.acceptance_criteria;
		if (f.priority) feedback.priority = f.priority;
		feedback.notice = t('feedback-voice-applied');
	} catch (err) {
		feedback.error = t('feedback-err-network', { error: String(err) });
	}
}

/** Stop and release the mic — the dialog closing must not leave it hot. */
export function stopVoice(): void {
	if (!recorder) return;
	const current = recorder;
	recorder = null;
	feedback.voicePhase = 'idle';
	void current.stop().catch(() => {});
}

// ---------------------------------------------------------------------------
// Diagnostics

/**
 * Everything the issue carries besides the form: where the reporter was, what
 * their browser is, which tools they had, and — unless they opted out — the
 * console/network ring buffers and the tail of the conversation.
 */
export function collectSystemInfo(): Record<string, unknown> {
	const segments = location.pathname.replace(base, '').split('/').filter(Boolean);
	const info: Record<string, unknown> = {
		url: location.href,
		module: segments[0] ?? '',
		timestamp: new Date().toISOString(),
		browser: navigator.userAgent,
		language: navigator.language,
		screen_resolution: `${window.screen.width}x${window.screen.height}`,
		viewport_size: `${window.innerWidth}x${window.innerHeight}`,
		device_pixel_ratio: window.devicePixelRatio
	};
	if (feedback.includeBrowserLog) {
		info.console_logs = getConsoleLogs();
		// The buffer holds 100; the last 50 are the ones that led here.
		info.network_logs = getNetworkLogs().slice(-50);
	}
	if (feedback.includeChatLog && isChatPage()) {
		info.allowed_tools = (me.value?.allowed_tools ?? []).map((tool) => tool.id || tool.name);
		const transcript = document.querySelector<HTMLElement>('[data-chat-transcript]');
		const text = (transcript?.innerText ?? '').trim();
		info.chat = {
			session_id: segments[1] ?? '',
			transcript_tail: text ? text.slice(-4000) : ''
		};
	}
	return info;
}

/** The chat-log consent only makes sense on a conversation page. */
export function isChatPage(): boolean {
	const segments = location.pathname.replace(base, '').split('/').filter(Boolean);
	return segments[0] === 'chat' && Boolean(segments[1]);
}

// ---------------------------------------------------------------------------
// Submit

/**
 * Validate, then ask for confirmation. Validation runs first on purpose: it
 * is rude to make someone confirm a public post and only then tell them the
 * title is too short.
 */
export function requestSubmit(): void {
	feedback.error = null;
	if (feedback.title.trim().length < 4) {
		feedback.error = t('feedback-err-title-required');
		return;
	}
	if (!feedback.description.trim()) {
		feedback.error = t('feedback-err-description-required');
		return;
	}
	feedback.confirming = true;
}

export function cancelConfirm(): void {
	feedback.confirming = false;
}

/** The actual POST. Only reached after the reporter confirms. */
export async function submit(): Promise<void> {
	if (feedback.busy) return;
	feedback.confirming = false;
	feedback.busy = true;
	feedback.error = null;
	try {
		const annotated = readAnnotatedShot?.() ?? null;
		const shot = annotated ?? feedback.shotDataUrl;
		const res = await fetch('/api/v0/feedback', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				title: feedback.title.trim(),
				description: feedback.description.trim(),
				business_value: feedback.business.trim(),
				acceptance_criteria: feedback.acceptance.trim(),
				priority: feedback.priority,
				screenshot_base64: shot ? stripDataUrlPrefix(shot) : undefined,
				attachments_base64: feedback.attachments.map((a) => a.base64),
				system_info: collectSystemInfo()
			})
		});
		if (!res.ok) {
			feedback.error = await errorMessage(res);
			return;
		}
		const data = (await res.json()) as { number?: number; url?: string };
		feedback.filed = { number: data.number ?? 0, url: data.url ?? '' };
		// Keep the dialog open on the thank-you panel; clear the draft so a
		// second report does not start from the first one's text.
		feedback.title = '';
		feedback.description = '';
		feedback.business = '';
		feedback.acceptance = '';
		feedback.priority = 'medium';
		removeShot();
		clearAttachments();
	} catch (err) {
		feedback.error = t('feedback-err-network', { error: String(err) });
	} finally {
		feedback.busy = false;
	}
}

/** Pull the server's own message out of a JSON error envelope. */
async function errorMessage(resp: Response): Promise<string> {
	const raw = await resp.text();
	try {
		const parsed = JSON.parse(raw) as { error?: { message?: string } };
		return (parsed?.error?.message || raw).slice(0, 200);
	} catch {
		return raw.slice(0, 200) || `request failed (${resp.status})`;
	}
}
