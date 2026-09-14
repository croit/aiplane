<script lang="ts">
	import { base } from '$app/paths';
	import { adminDelete, adminPost } from '$lib/admin-client';
	import { locale, t } from '$lib/i18n.svelte';
	import { runLinks } from '$lib/run-links';
	import { formatWebhookFire, type Webhook } from '$lib/webhooks';

	/**
	 * One webhook on `/webhooks`.
	 *
	 * The card answers the two questions the page exists for: is this hook
	 * live, and where is what it produced. The second one used to be a link on
	 * the last fire alone — `runLinks` picks between the one conversation and
	 * the list of them, by the same rule `/scheduled` uses.
	 */
	let { webhook, onchanged, onnotice, onsecret } = $props<{
		webhook: Webhook;
		onchanged: () => void | Promise<void>;
		onnotice: (message: string) => void;
		onsecret: (secret: string) => void;
	}>();

	let links = $derived(runLinks(webhook, '/webhooks'));

	async function toggle() {
		try {
			await adminPost(`/api/v0/webhooks/${webhook.id}/toggle`, { enabled: !webhook.enabled });
			await onchanged();
		} catch (caught) {
			onnotice(String(caught));
		}
	}

	async function rotate() {
		if (!confirm(t('webhooks-rotate-confirm'))) return;
		try {
			onsecret((await adminPost<{ secret: string }>(`/api/v0/webhooks/${webhook.id}/rotate`)).secret);
		} catch (caught) {
			onnotice(String(caught));
		}
	}

	async function remove() {
		if (!confirm(t('webhooks-delete-confirm'))) return;
		try {
			await adminDelete(`/api/v0/webhooks/${webhook.id}`);
			await onchanged();
		} catch (caught) {
			onnotice(String(caught));
		}
	}
</script>

<article class="card card-border bg-base-100">
	<div class="card-body gap-4">
		<div class="flex flex-col gap-3 sm:flex-row sm:items-start">
			<div class="min-w-0 flex-1">
				<div class="flex flex-wrap items-center gap-2">
					<h3 class="card-title text-base">{webhook.name}</h3>
					<span class="badge badge-sm {webhook.enabled ? 'badge-success' : 'badge-ghost'}">{webhook.enabled ? t('webhooks-badge-active') : t('webhooks-badge-paused')}</span>
					{#if webhook.reuse_conversation}<span class="badge badge-ghost badge-sm">{t('webhooks-badge-reuses-chat')}</span>{/if}
				</div>
				<p class="mt-1 line-clamp-2 text-sm text-base-content/70">{webhook.prompt}</p>
				<div class="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-xs text-base-content/60">
					<span class="font-mono">{webhook.model}</span>
					<span>·</span>
					<span>{t(webhook.synchronous ? 'webhooks-mode-sync' : 'webhooks-mode-async')}</span>
					<span>·</span>
					<span class="font-mono">/hooks/gwh_••••••••••••</span>
				</div>
				{#if webhook.last_fired_at}
					<p class="mt-1 text-xs {webhook.last_status === 'ok' ? 'text-success' : 'text-error'}">
						{t(webhook.last_status === 'ok' ? 'webhooks-last-success' : 'webhooks-last-failure', { when: formatWebhookFire(webhook.last_fired_at, locale.current) })}
					</p>
				{/if}
				{#if webhook.last_error}<p class="mt-1 text-xs text-error">{webhook.last_error}</p>{/if}
			</div>
			<div class="flex flex-wrap gap-2">
				<a class="btn btn-sm" href="{base}/webhooks/{webhook.id}/edit">{t('webhooks-edit-title')}</a>
				<button class="btn btn-sm" type="button" onclick={toggle}>{webhook.enabled ? t('webhooks-pause-title') : t('webhooks-resume-title')}</button>
				<button class="btn btn-sm" type="button" onclick={rotate}>{t('webhooks-rotate-title')}</button>
				<button class="btn btn-outline btn-error btn-sm" type="button" onclick={remove}>{t('webhooks-delete-title')}</button>
			</div>
		</div>

		<!-- What this webhook produced. A hook that has never fired says so
		     rather than showing dead links. -->
		<div class="flex flex-wrap items-center gap-3 border-t border-base-300 pt-3 text-sm">
			{#if links.chat}
				<a class="link link-hover font-medium" href="{base}{links.chat}">{t('webhooks-open-chat')}</a>
				{#if links.runs}<a class="link link-hover text-base-content/70" href="{base}{links.runs}">{t('webhooks-open-runs', { count: webhook.run_count })}</a>{/if}
			{:else if links.runs}
				<a class="link link-hover font-medium" href="{base}{links.runs}">
					{webhook.chat_count > 0 ? t('webhooks-open-chats', { count: webhook.chat_count }) : t('webhooks-open-runs', { count: webhook.run_count })}
				</a>
			{:else}
				<span class="text-base-content/50">{t('webhooks-never-fired')}</span>
			{/if}
			{#if webhook.has_payload}<a class="link link-hover text-base-content/70" href="{base}/webhooks/{webhook.id}/rerun">{t('webhooks-rerun-link')}</a>{/if}
		</div>
	</div>
</article>
