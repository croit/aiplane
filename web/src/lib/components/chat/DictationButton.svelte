<script lang="ts">
	import { base } from '$app/paths';
	import { t } from '$lib/i18n.svelte';
	import {
		recordingErrorMessage,
		recordingUnavailableReason,
		startRecording,
		type VoiceRecorder
	} from '$lib/voice-recorder';

	let { model, ontranscript, onerror }: {
		model: string;
		ontranscript: (text: string) => void;
		onerror: (message: string) => void;
	} = $props();

	let recorder: VoiceRecorder | null = null;
	let phase = $state<'idle' | 'recording' | 'transcribing'>('idle');

	async function transcribe(wav: Blob) {
		if (!wav.size) {
			phase = 'idle';
			return;
		}
		phase = 'transcribing';
		const form = new FormData();
		form.append('model', model);
		form.append('file', wav, 'recording.wav');
		try {
			const response = await fetch('/api/v0/transcriptions', { method: 'POST', body: form });
			if (!response.ok) {
				const raw = await response.text();
				let message = raw;
				try {
					message = (JSON.parse(raw) as { error?: { message?: string } }).error?.message ?? raw;
				} catch {
					/* The response is already useful plain text. */
				}
				throw new Error(message.slice(0, 200));
			}
			const text = ((await response.json()) as { text?: string }).text?.trim() ?? '';
			if (text) ontranscript(text);
			else onerror(t('voice-not-caught'));
		} catch (caught) {
			onerror(t('voice-network-error', { error: String(caught) }));
		} finally {
			phase = 'idle';
		}
	}

	async function toggle() {
		if (phase === 'transcribing') return;
		if (recorder) {
			const current = recorder;
			recorder = null;
			try {
				await transcribe(await current.stop());
			} catch (caught) {
				phase = 'idle';
				onerror(t('voice-recording-stop-failed', { error: String(caught) }));
			}
			return;
		}
		const unavailable = recordingUnavailableReason();
		if (unavailable) {
			onerror(unavailable);
			return;
		}
		try {
			recorder = await startRecording(`${base}/pcm-recorder.js`);
			phase = 'recording';
		} catch (caught) {
			onerror(recordingErrorMessage(caught));
		}
	}
</script>

<button
	type="button"
	class="btn btn-sm btn-circle btn-ghost {phase === 'recording' ? 'btn-error' : ''}"
	onclick={toggle}
	disabled={phase === 'transcribing'}
	aria-label={t('render-composer-record-aria')}
	title={t('render-composer-record-title')}
>
	{#if phase === 'transcribing'}
		<span class="loading loading-spinner loading-xs"></span>
	{:else if phase === 'recording'}
		<svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="1" /></svg>
	{:else}
		<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" aria-hidden="true"><rect x="9" y="2" width="6" height="11" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1M12 18v3M8 22h8" /></svg>
	{/if}
</button>
