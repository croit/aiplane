<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { api, type DotBrowserInput } from '#lib/api.js';
	import { t } from '#lib/i18n.svelte.js';
	import type { SuspensionView } from '#lib/suspension.js';

	let { sessionId, turnId, waiting, oncomplete }: {
		sessionId: string;
		turnId: string;
		waiting: SuspensionView;
		oncomplete: () => void;
	} = $props();

	let controllerId = $state<string | null>(null);
	let imageUrl = $state<string | null>(null);
	let frameSize = $state<{ width: number; height: number } | null>(null);
	let expiresAt = $state('');
	let error = $state(false);
	let busy = $state(false);
	let text = $state('');
	let viewport = $state<HTMLImageElement | null>(null);
	let sequence = 0;
	let stopped = false;
	let timer: ReturnType<typeof setTimeout> | undefined;
	let clockTimer: ReturnType<typeof setInterval> | undefined;
	let remaining = $state('');

	function updateRemaining() {
		const minutes = Math.max(0, Math.ceil((Date.parse(expiresAt) - Date.now()) / 60000));
		remaining = t('suspension-expires-in', { minutes });
	}

	function setFrame(frame: { frame_b64: string | null; width: number | null; height: number | null; expires_at: string }) {
		expiresAt = frame.expires_at;
		if (frame.frame_b64) {
			const bytes = Uint8Array.from(atob(frame.frame_b64), (char) => char.charCodeAt(0));
			const nextUrl = URL.createObjectURL(new Blob([bytes], { type: 'image/jpeg' }));
			if (imageUrl) URL.revokeObjectURL(imageUrl);
			imageUrl = nextUrl;
			if (frame.width && frame.height) frameSize = { width: frame.width, height: frame.height };
		}
		updateRemaining();
	}

	async function refreshFrame() {
		if (stopped || !controllerId) return;
		try {
			setFrame(await api.dotBrowserFrame(sessionId, turnId, waiting.request_id, controllerId));
			error = false;
		} catch {
			error = true;
		} finally {
			if (!stopped) timer = setTimeout(refreshFrame, 900);
		}
	}

	async function send(input: DotBrowserInput): Promise<boolean> {
		if (!controllerId || busy || stopped) return false;
		busy = true;
		try {
			const result = await api.dotBrowserInput(sessionId, turnId, waiting.request_id, controllerId, ++sequence, input);
			expiresAt = result.expires_at;
			updateRemaining();
			return true;
		} catch {
			error = true;
			return false;
		} finally {
			busy = false;
		}
	}

	function clickFrame(event: MouseEvent) {
		if (!viewport || !frameSize) return;
		const bounds = viewport.getBoundingClientRect();
		const x = Math.max(0, Math.min(frameSize.width - 1, Math.round((event.clientX - bounds.left) * frameSize.width / bounds.width)));
		const y = Math.max(0, Math.min(frameSize.height - 1, Math.round((event.clientY - bounds.top) * frameSize.height / bounds.height)));
		void send({ kind: 'click', x, y });
	}

	function scrollFrame(event: WheelEvent) {
		event.preventDefault();
		void send({ kind: 'scroll', delta_x: Math.round(event.deltaX), delta_y: Math.round(event.deltaY) });
	}

	function keyFrame(event: KeyboardEvent) {
		if (!['Tab', 'Enter', 'Escape', 'Backspace', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown'].includes(event.key)) return;
		event.preventDefault();
		void send({ kind: 'key', key: event.key });
	}

	async function submitText(event: SubmitEvent) {
		event.preventDefault();
		if (!text.trim()) return;
		if (await send({ kind: 'text', text })) text = '';
	}

	async function finish() {
		if (!controllerId || busy) return;
		busy = true;
		stopped = true;
		if (timer) clearTimeout(timer);
		try {
			await api.dotBrowserFinish(sessionId, turnId, waiting.request_id, controllerId);
			oncomplete();
		} catch {
			error = true;
			stopped = false;
		} finally {
			busy = false;
		}
	}

	async function abort() {
		if (!controllerId || busy) return;
		busy = true;
		stopped = true;
		if (timer) clearTimeout(timer);
		try {
			await api.dotBrowserAbort(sessionId, turnId, waiting.request_id, controllerId);
			oncomplete();
		} catch {
			error = true;
			stopped = false;
		} finally {
			busy = false;
		}
	}

	onMount(() => {
		clockTimer = setInterval(updateRemaining, 15000);
		void (async () => {
			try {
				const claim = await api.dotBrowserClaim(sessionId, turnId, waiting.request_id);
				controllerId = claim.controller_id;
				expiresAt = claim.expires_at;
				await refreshFrame();
			} catch {
				error = true;
			}
		})();
		return () => {
			stopped = true;
			if (timer) clearTimeout(timer);
			if (clockTimer) clearInterval(clockTimer);
			if (imageUrl) URL.revokeObjectURL(imageUrl);
		};
	});

	onDestroy(() => {
		stopped = true;
		if (imageUrl) URL.revokeObjectURL(imageUrl);
	});
</script>

<section class="card card-border bg-base-200 p-4 sm:p-5" aria-label={t('chat-waiting-browser-handover')}>
	<div class="mb-3 flex flex-wrap items-center justify-between gap-2">
	<h3 class="m-0 text-base font-semibold">{t('chat-waiting-browser-handover')}</h3>
		{#if remaining}<span class="badge badge-outline">{remaining}</span>{/if}
	</div>
	<p class="mb-4 text-sm text-base-content/70">{t('suspension-browser-instructions')}</p>
	<div class="mockup-browser border border-base-300 bg-base-100">
		<div class="mockup-browser-toolbar"><div class="input">{t('suspension-browser-private')}</div></div>
		<div class="flex min-h-48 items-center justify-center bg-base-200 p-2 sm:min-h-80">
			{#if imageUrl}
				<button type="button" aria-label={t('suspension-browser-frame-alt')} class="btn btn-ghost h-auto min-h-0 max-w-full p-0 hover:bg-transparent" onclick={clickFrame} onwheel={scrollFrame} onkeydown={keyFrame}>
					<img bind:this={viewport} src={imageUrl} alt="" class="max-h-[65vh] max-w-full object-contain" />
				</button>
			{:else}
				<span class="loading loading-spinner loading-md" aria-label={t('suspension-browser-loading')}></span>
			{/if}
		</div>
	</div>
	{#if error}<div class="alert alert-error mt-3 py-2" role="alert">{t('suspension-browser-error')}</div>{/if}
	<form class="mt-3 flex gap-2" onsubmit={submitText}>
		<input class="input min-w-0 flex-1" bind:value={text} maxlength="512" placeholder={t('suspension-browser-text-placeholder')} aria-label={t('suspension-browser-text-label')} disabled={!controllerId || busy} />
		<button class="btn btn-sm" type="submit" disabled={!text.trim() || busy}>{t('suspension-browser-send')}</button>
	</form>
	<div class="mt-4 flex flex-wrap justify-end gap-2">
		<button class="btn btn-sm btn-ghost" type="button" onclick={abort} disabled={!controllerId || busy}>{t('suspension-browser-stop')}</button>
		<button class="btn btn-sm btn-primary" type="button" onclick={finish} disabled={!controllerId || busy}>{t('suspension-browser-return')}</button>
	</div>
</section>
