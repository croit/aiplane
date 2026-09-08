/**
 * In-browser voice recorder for the SPA (issue #22 P2) — the port of the
 * legacy `ui/ts/voice-recorder.ts`: raw PCM via Web Audio + AudioWorklet,
 * resampled to 16 kHz mono, encoded as a canonical 44-byte WAV. PCM rather
 * than MediaRecorder/Opus because the `/api/v0/transcriptions` handler runs
 * the upload through a neural VAD (earshot) that needs raw PCM16 — see
 * `crates/gateway/src/rama_server/vad.rs`.
 *
 * Differences from the legacy module: the worklet URL is an explicit
 * parameter (the SPA serves it at `${base}/pcm-recorder.js`) and the
 * DOM-level meter tap is dropped (the SPA modal animates off CSS state).
 */

export const TARGET_RATE = 16000;

const resampleTo16k = (samples: Float32Array, fromRate: number): Float32Array => {
	if (fromRate === TARGET_RATE) return samples;
	const ratio = fromRate / TARGET_RATE;
	const outLen = Math.floor(samples.length / ratio);
	const out = new Float32Array(outLen);
	for (let i = 0; i < outLen; i++) {
		const src = i * ratio;
		const lo = Math.floor(src);
		const hi = Math.min(lo + 1, samples.length - 1);
		const t = src - lo;
		out[i] = samples[lo]! * (1 - t) + samples[hi]! * t;
	}
	return out;
};

/**
 * Pack a Float32 sample buffer (range -1..1) into a 16 kHz mono 16-bit PCM
 * WAV (44-byte canonical header). Matches what
 * `rama_server::vad::parse_pcm16_mono_16k` expects.
 */
export const encodeWav = (samples: Float32Array): Blob => {
	const numSamples = samples.length;
	const dataSize = numSamples * 2;
	const buf = new ArrayBuffer(44 + dataSize);
	const view = new DataView(buf);
	const writeStr = (offset: number, s: string): void => {
		for (let i = 0; i < s.length; i++) view.setUint8(offset + i, s.charCodeAt(i));
	};
	writeStr(0, 'RIFF');
	view.setUint32(4, 36 + dataSize, true);
	writeStr(8, 'WAVE');
	writeStr(12, 'fmt ');
	view.setUint32(16, 16, true);
	view.setUint16(20, 1, true);
	view.setUint16(22, 1, true);
	view.setUint32(24, TARGET_RATE, true);
	view.setUint32(28, TARGET_RATE * 2, true);
	view.setUint16(32, 2, true);
	view.setUint16(34, 16, true);
	writeStr(36, 'data');
	view.setUint32(40, dataSize, true);
	let offset = 44;
	for (let i = 0; i < numSamples; i++) {
		const s = Math.max(-1, Math.min(1, samples[i]!));
		view.setInt16(offset, s < 0 ? s * 0x8000 : s * 0x7fff, true);
		offset += 2;
	}
	return new Blob([buf], { type: 'audio/wav' });
};

/** One recording session: AudioContext, mic stream, worklet, chunk buffer. */
class VoiceRecorder {
	private readonly chunks: Float32Array[] = [];
	private readonly captureRate: number;

	constructor(
		private readonly ctx: AudioContext,
		private readonly stream: MediaStream,
		private readonly source: MediaStreamAudioSourceNode,
		private readonly node: AudioWorkletNode
	) {
		this.captureRate = ctx.sampleRate;
		this.node.port.onmessage = (e: MessageEvent) => {
			if (e.data instanceof Float32Array) this.chunks.push(e.data);
		};
	}

	/** Stop capture, tear everything down, return the encoded WAV. */
	async stop(): Promise<Blob> {
		try {
			this.node.port.onmessage = null;
			this.source.disconnect();
			this.node.disconnect();
		} catch {
			// best-effort tear-down
		} finally {
			// Releasing the mic and closing the context must happen even if a
			// disconnect above threw — otherwise the mic stays hot.
			try {
				this.stream.getTracks().forEach((t) => t.stop());
			} catch {
				/* best-effort */
			}
			try {
				await this.ctx.close();
			} catch {
				/* best-effort */
			}
		}
		let total = 0;
		for (const c of this.chunks) total += c.length;
		if (total === 0) return new Blob([], { type: 'audio/wav' });
		const flat = new Float32Array(total);
		let off = 0;
		for (const c of this.chunks) {
			flat.set(c, off);
			off += c.length;
		}
		return encodeWav(resampleTo16k(flat, this.captureRate));
	}
}

export type { VoiceRecorder };

/** Capability check — runs at click time so a fresh mount re-verifies. */
export const recordingUnavailableReason = (): string | null => {
	if (!navigator.mediaDevices || !window.isSecureContext) {
		return 'Voice recording requires HTTPS or localhost — disabled on plain http.';
	}
	if (!(window.AudioContext && 'audioWorklet' in AudioContext.prototype)) {
		return 'Voice recording requires AudioWorklet support.';
	}
	return null;
};

/** Start a recording session against the worklet at `workletUrl`. */
export const startRecording = async (workletUrl: string): Promise<VoiceRecorder> => {
	const stream = await navigator.mediaDevices.getUserMedia({
		audio: {
			echoCancellation: true,
			noiseSuppression: true,
			autoGainControl: true,
			channelCount: 1,
			sampleRate: TARGET_RATE
		}
	});
	const ctx = new AudioContext({ sampleRate: TARGET_RATE });
	try {
		await ctx.audioWorklet.addModule(workletUrl);
	} catch (err) {
		await ctx.close().catch(() => {});
		stream.getTracks().forEach((t) => t.stop());
		throw err;
	}
	const source = ctx.createMediaStreamSource(stream);
	const node = new AudioWorkletNode(ctx, 'pcm-recorder');
	// The processor only runs when there's an active path to the destination;
	// route it through a muted gain so `process()` is scheduled without
	// playing the mic back.
	source.connect(node);
	const muted = ctx.createGain();
	muted.gain.value = 0;
	node.connect(muted).connect(ctx.destination);
	return new VoiceRecorder(ctx, stream, source, node);
};

export const recordingErrorMessage = (err: unknown): string => {
	const name = (err instanceof Error && err.name) || 'Error';
	if (name === 'NotAllowedError' || name === 'PermissionDeniedError') {
		return 'Microphone access denied. Allow it in the browser and retry.';
	}
	if (name === 'NotFoundError' || name === 'DevicesNotFoundError') {
		return 'No microphone found.';
	}
	if (name === 'NotReadableError') {
		return 'Microphone is busy — another app may be using it.';
	}
	const message = err instanceof Error ? err.message : String(err);
	return `Mic error: ${message}`;
};
