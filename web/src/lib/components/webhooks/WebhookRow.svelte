<script lang="ts">
	import { adminDelete, adminPost } from '$lib/admin-client';
	import { locale, t } from '$lib/i18n.svelte';
	import { formatWebhookFire, type Webhook } from '$lib/webhooks';

	let { webhook, onchanged, onnotice, onsecret } = $props<{
		webhook: Webhook;
		onchanged: () => void | Promise<void>;
		onnotice: (message: string) => void;
		onsecret: (secret: string) => void;
	}>();
	async function toggle() {
		try { await adminPost(`/api/v0/webhooks/${webhook.id}/toggle`, { enabled: !webhook.enabled }); await onchanged(); }
		catch (caught) { onnotice(String(caught)); }
	}
	async function rotate() {
		if (!confirm(t('webhooks-rotate-confirm'))) return;
		try { onsecret((await adminPost<{ secret: string }>(`/api/v0/webhooks/${webhook.id}/rotate`)).secret); }
		catch (caught) { onnotice(String(caught)); }
	}
	async function remove() {
		if (!confirm(t('webhooks-delete-confirm'))) return;
		try { await adminDelete(`/api/v0/webhooks/${webhook.id}`); await onchanged(); }
		catch (caught) { onnotice(String(caught)); }
	}
</script>

<li class="flex flex-col gap-3 py-3 sm:flex-row sm:items-start">
	<div class="min-w-0 flex-1">
		<div class="flex items-center gap-2"><strong class="text-sm">{webhook.name}</strong><span class="badge badge-sm {webhook.enabled ? 'badge-success' : 'badge-ghost'}">{webhook.enabled ? t('webhooks-badge-active') : t('webhooks-badge-paused')}</span></div>
		<p class="truncate text-xs text-base-content/60">{webhook.prompt}</p>
		<p class="mt-0.5 text-xs text-base-content/70">{webhook.model} · {t(webhook.synchronous ? 'webhooks-mode-sync' : 'webhooks-mode-async')}</p>
		<p class="mt-0.5 truncate font-mono text-xs text-base-content/60">/hooks/gwh_••••••••••••</p>
		<div class="mt-0.5 flex flex-wrap items-center gap-x-3 text-xs text-base-content/60">
			{#if webhook.last_fired_at}
				{#if webhook.last_session_id}<a class="link link-hover {webhook.last_status === 'ok' ? 'text-success' : 'text-error'}" href={`/chat/${webhook.last_session_id}`}>{t(webhook.last_status === 'ok' ? 'webhooks-last-success-open' : 'webhooks-last-failure-open', { when: formatWebhookFire(webhook.last_fired_at, locale.current) })}</a>
				{:else}<span class={webhook.last_status === 'ok' ? 'text-success' : 'text-error'}>{t(webhook.last_status === 'ok' ? 'webhooks-last-success' : 'webhooks-last-failure', { when: formatWebhookFire(webhook.last_fired_at, locale.current) })}</span>{/if}
			{:else}<span>{t('webhooks-never-fired')}</span>{/if}
			{#if webhook.has_payload}<a class="link link-hover" href={`/webhooks/${webhook.id}/rerun`}>{t('webhooks-rerun-link')}</a>{/if}
			{#if webhook.last_fired_at}<a class="link link-hover" href={`/webhooks/${webhook.id}/runs`}>{t('webhooks-runs-link')}</a>{/if}
		</div>
		{#if webhook.last_error}<p class="mt-1 text-xs text-error">{webhook.last_error}</p>{/if}
	</div>
	<div class="flex shrink-0 items-center gap-1 self-end sm:self-auto">
		<button class="btn btn-ghost btn-sm" type="button" onclick={toggle}>{webhook.enabled ? t('webhooks-pause-title') : t('webhooks-resume-title')}</button>
		<button class="btn btn-ghost btn-sm" type="button" onclick={rotate}>{t('webhooks-rotate-title')}</button>
		<a class="btn btn-ghost btn-sm" href={`/webhooks/${webhook.id}/edit`}>{t('webhooks-edit-title')}</a>
		<button class="btn btn-ghost btn-error btn-sm" type="button" onclick={remove}>{t('webhooks-delete-title')}</button>
	</div>
</li>
