/**
 * Web Push opt-in for the SPA (issue #22 P2) — the reactive port of the
 * legacy `ui/ts/push.ts`: reflect this *browser's* subscription state and
 * drive subscribe/unsubscribe through `/api/v0/push/*` + the SPA service
 * worker (`static/sw.js`, scope `/`).
 *
 * State here is device-local, not server state — two browsers of the same
 * user subscribe independently.
 */
import { t } from './i18n.svelte';

interface PushConfig {
	enabled: boolean;
	publicKey: string | null;
}

export type PushUiState =
	| { phase: 'unsupported' }
	| { phase: 'denied' }
	| { phase: 'off' }
	| { phase: 'on' }
	| { phase: 'busy' };

function supported(): boolean {
	return 'serviceWorker' in navigator && 'PushManager' in window && 'Notification' in window;
}

/** Decode a base64url VAPID key into the `applicationServerKey` BufferSource. */
function urlB64ToUint8Array(base64: string): Uint8Array<ArrayBuffer> {
	const padding = '='.repeat((4 - (base64.length % 4)) % 4);
	const b64 = (base64 + padding).replace(/-/g, '+').replace(/_/g, '/');
	const raw = atob(b64);
	const out = new Uint8Array(new ArrayBuffer(raw.length));
	for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
	return out;
}

export const push = $state<{ ui: PushUiState; note: string | null }>({
	ui: { phase: 'off' },
	note: null
});

async function currentSubscription(): Promise<PushSubscription | null> {
	if (!supported()) return null;
	const reg = await navigator.serviceWorker.ready;
	return reg.pushManager.getSubscription();
}

export async function refreshPushState(): Promise<void> {
	if (!supported()) {
		push.ui = { phase: 'unsupported' };
		return;
	}
	if (Notification.permission === 'denied') {
		push.ui = { phase: 'denied' };
		return;
	}
	try {
		push.ui = (await currentSubscription()) ? { phase: 'on' } : { phase: 'off' };
	} catch {
		push.ui = { phase: 'off' };
	}
}

export async function enablePush(): Promise<void> {
	if (!supported()) return;
	push.ui = { phase: 'busy' };
	try {
		const cfgRes = await fetch('/api/v0/push/config', { headers: { accept: 'application/json' } });
		const cfg = (await cfgRes.json()) as PushConfig;
		if (!cfg.enabled || !cfg.publicKey) {
			push.ui = { phase: 'unsupported' };
			return;
		}
		const permission = await Notification.requestPermission();
		if (permission !== 'granted') {
			push.ui = { phase: 'denied' };
			return;
		}
		const reg = await navigator.serviceWorker.ready;
		let sub = await reg.pushManager.getSubscription();
		if (!sub) {
			sub = await reg.pushManager.subscribe({
				userVisibleOnly: true,
				applicationServerKey: urlB64ToUint8Array(cfg.publicKey)
			});
		}
		const res = await fetch('/api/v0/push/subscribe', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(sub.toJSON())
		});
		if (!res.ok) throw new Error(`subscribe ${res.status}`);
		push.note = t('tokens-push-enabled');
	} catch {
		push.note = t('tokens-push-error');
	} finally {
		await refreshPushState();
	}
}

export async function disablePush(): Promise<void> {
	if (!supported()) return;
	push.ui = { phase: 'busy' };
	try {
		const sub = await currentSubscription();
		if (sub) {
			const endpoint = sub.endpoint;
			await sub.unsubscribe();
			await fetch('/api/v0/push/unsubscribe', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ endpoint })
			});
		}
		push.note = t('tokens-push-disabled');
	} catch {
		push.note = t('tokens-push-error');
	} finally {
		await refreshPushState();
	}
}
