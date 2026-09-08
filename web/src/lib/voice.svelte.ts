/**
 * Voice-conversation mode for the SPA (issue #22 P2) — the port of the
 * legacy `ui/ts/chat/voice.ts`, adapted to the reactive store instead of
 * DOM MutationObservers: the conversation page feeds the live assistant
 * text straight in (`feedReplyText`), and this controller peels finished
 * sentences off it for low-latency TTS.
 *
 * The turn itself is the ordinary JSON submit with `voice: true` — the
 * worker injects the spoken-style directive; the reply streams on the same
 * events endpoint as a typed turn.
 *
 * Loop: tap to talk → tap to stop + transcribe (PCM WAV →
 * /api/v0/transcriptions) → submit → speak the reply sentence-by-sentence
 * (POST /api/v0/speech → play) → tap while speaking interrupts and frees
 * the mic again.
 */
import { base } from '$app/paths';
import { api } from './api';
import {
	recordingErrorMessage,
	recordingUnavailableReason,
	startRecording,
	type VoiceRecorder
} from './voice-recorder';

export type VoiceState = 'idle' | 'listening' | 'working' | 'speaking';

interface SpeechResponseInfo {
	/** Whether /api/v0/speech answered with audio (vs. a 204 no-content). */
	hadAudio: boolean;
}

export function createVoiceController(submit: (text: string) => Promise<void>) {
	const state = $state<{ phase: VoiceState; captionUser: string; captionAi: string; note: string | null }>({
		phase: 'idle',
		captionUser: '',
		captionAi: '',
		note: null
	});

	let recorder: VoiceRecorder | null = null;
	let spokenChars = 0;
	let spokenLang = '';
	const ttsQueue: string[] = [];
	let playing = false;

	const audio = new Audio();
	const playNext = async (): Promise<void> => {
		const sentence = ttsQueue.shift();
		if (!sentence) {
			playing = false;
			if (state.phase === 'speaking') state.phase = 'idle';
			return;
		}
		playing = true;
		state.phase = 'speaking';
		try {
			const res = await fetch('/api/v0/speech', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ text: sentence, language: spokenLang || undefined })
			});
			if (res.ok && res.status !== 204) {
				const blob = await res.blob();
				const url = URL.createObjectURL(blob);
				await new Promise<void>((resolve) => {
					audio.src = url;
					audio.onended = () => resolve();
					audio.onerror = () => resolve();
					void audio.play().catch(() => resolve());
				});
				URL.revokeObjectURL(url);
			}
		} catch {
			/* a failed chunk must not stall the queue */
		}
		void playNext();
	};

	const enqueueSpeech = (sentence: string): void => {
		if (!sentence.trim()) return;
		ttsQueue.push(sentence);
		if (!playing) void playNext();
	};

	const stopPlayback = (): void => {
		ttsQueue.length = 0;
		audio.pause();
		audio.src = '';
		playing = false;
		if (state.phase === 'speaking') state.phase = 'idle';
	};

	/** Peel finished sentences off freshly streamed reply text. */
	const splitSentences = (text: string): { done: string[]; rest: string } => {
		const done: string[] = [];
		const re = /[^.!?…]*[.!?…]+["')\]]*\s+/g;
		let last = 0;
		let m: RegExpExecArray | null;
		while ((m = re.exec(text)) !== null) {
			done.push(m[0].trim());
			last = re.lastIndex;
		}
		return { done, rest: text.slice(last) };
	};

	/**
	 * Feed the live assistant text. Called by the page on every content
	 * change of the voice-submitted turn (and once with `final: true`).
	 */
	const feedReplyText = (text: string, final: boolean): void => {
		if (state.phase === 'working') state.phase = 'speaking';
		state.captionAi = text;
		if (text.length < spokenChars) {
			// Rewritten (a retry regenerated the row): restart from scratch.
			spokenChars = 0;
		}
		if (text.length <= spokenChars) {
			if (final) stopFeeding();
			return;
		}
		const fresh = text.slice(spokenChars);
		const { done, rest } = splitSentences(fresh);
		for (const s of done) enqueueSpeech(s);
		spokenChars += fresh.length - rest.length;
		if (final) {
			if (rest.trim()) {
				enqueueSpeech(rest);
				spokenChars += rest.length;
			}
			stopFeeding();
		}
	};

	const stopFeeding = (): void => {
		if (state.phase === 'working') state.phase = 'idle';
	};

	/** Reset the reply tracking for a fresh voice turn. */
	const beginReply = (): void => {
		spokenChars = 0;
	};

	const transcribeAndSubmit = async (wav: Blob, model: string): Promise<void> => {
		state.phase = 'working';
		const fd = new FormData();
		fd.append('model', model);
		fd.append('file', wav, 'recording.wav');
		try {
			const resp = await fetch('/api/v0/transcriptions', { method: 'POST', body: fd });
			if (!resp.ok) {
				const raw = await resp.text();
				let msg = raw;
				try {
					msg = (JSON.parse(raw) as { error?: { message?: string } })?.error?.message || raw;
				} catch {
					/* raw */
				}
				state.note = msg.slice(0, 200);
				state.phase = 'idle';
				return;
			}
			const data = (await resp.json()) as { text?: string; language?: string };
			const text = (data.text ?? '').trim();
			if (!text) {
				state.note = 'Nothing caught — try again.';
				state.phase = 'idle';
				return;
			}
			spokenLang = (data.language ?? '').toLowerCase().slice(0, 2);
			state.captionUser = text;
			state.captionAi = '';
			beginReply();
			await submit(text);
			state.phase = 'working';
		} catch (err) {
			state.note = `network error: ${err}`;
			state.phase = 'idle';
		}
	};

	/** The one control: tap-to-talk semantics driven by the current state. */
	const tap = async (model: string): Promise<void> => {
		state.note = null;
		if (recorder) {
			const current = recorder;
			recorder = null;
			state.phase = 'working';
			let wav: Blob;
			try {
				wav = await current.stop();
			} catch (err) {
				state.note = `recording stop failed: ${err}`;
				state.phase = 'idle';
				return;
			}
			if (wav.size) await transcribeAndSubmit(wav, model);
			else state.phase = 'idle';
			return;
		}
		if (playing) {
			stopPlayback();
			return;
		}
		const unavailable = recordingUnavailableReason();
		if (unavailable) {
			state.note = unavailable;
			return;
		}
		try {
			recorder = await startRecording(`${base}/pcm-recorder.js`);
			state.phase = 'listening';
		} catch (err) {
			state.note = recordingErrorMessage(err);
		}
	};

	const close = (): void => {
		if (recorder) {
			void recorder.stop();
			recorder = null;
		}
		stopPlayback();
		state.phase = 'idle';
	};

	return { state, tap, close, feedReplyText };
}

export type VoiceController = ReturnType<typeof createVoiceController>;
export type { SpeechResponseInfo };
