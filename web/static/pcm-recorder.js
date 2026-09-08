// croit LLM Gateway — AudioWorklet PCM recorder processor (SPA copy).
//
// Served at /app/pcm-recorder.js (web/static is copied verbatim by the
// SvelteKit build) and loaded via `audioWorklet.addModule()` from
// web/src/lib/voice-recorder.ts. This is the same processor the legacy UI
// ships at /assets/pcm-recorder.js (compiled from ui/ts/pcm-recorder.ts) —
// keep the two behaviour-compatible: the main-thread side (chunk
// accumulation + WAV encode) only understands this message shape.

class PcmRecorder extends AudioWorkletProcessor {
  process(inputs) {
    const channel = inputs[0] && inputs[0][0];
    if (channel && channel.length) {
      // .slice() because the underlying buffer is reused by the graph on
      // the next quantum — without the copy the main thread would observe
      // overwritten samples.
      this.port.postMessage(channel.slice());
    }
    return true;
  }
}

registerProcessor('pcm-recorder', PcmRecorder);
