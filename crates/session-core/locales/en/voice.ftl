# Strings for the voice-conversation mode (session-core render + gateway
# speech path). The `voice-*-marker` values are SPOKEN aloud in place of
# non-speakable content, so keep them natural sentences, not UI labels.

voice-code-marker = The code is shown on screen.
voice-table-marker = A table is shown on screen.
voice-greeting = Hi — just ask me anything.
voice-toggle-title = Start voice conversation
voice-exit-title = End voice conversation
voice-ptt-title = Hold to talk
voice-status-listening = Listening…
voice-status-working = Working…
voice-status-speaking = Speaking…
voice-unavailable = Voice output isn't configured on this server.
voice-modal-title = Voice conversation
voice-hint-tap-to-talk = Tap to talk
voice-hint-tap-to-send = Tap to send
voice-hint-tap-to-interrupt = Tap to interrupt
voice-recording-to-chat = recording to chat
voice-caption-you = You
voice-caption-ai = AI
voice-not-caught = Didn’t catch that — try again.

# The voice modal's phase line (status + what a tap does), and the two
# failures the controller surfaces in its own notice area.
voice-phase-listening = Listening — tap to send
voice-phase-speaking = Speaking — tap to interrupt
voice-recording-stop-failed = Recording could not be stopped: { $error }
voice-network-error = Network error: { $error }

# Why the microphone would not start. Shown at the moment someone is
# trying to talk to the gateway, so they need it in their own language.
voice-mic-insecure-context = Voice recording requires HTTPS or localhost — disabled on plain http.
voice-mic-no-worklet = Voice recording requires AudioWorklet support.
voice-mic-denied = Microphone access denied. Allow it in the browser and retry.
voice-mic-not-found = No microphone found.
voice-mic-busy = Microphone is busy — another app may be using it.
voice-mic-error = Mic error: { $error }
