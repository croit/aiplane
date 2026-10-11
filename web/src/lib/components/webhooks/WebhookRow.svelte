<script lang="ts">
	import { onDestroy } from 'svelte';
	import { locale, t } from '#lib/i18n.svelte.js';
	import DotAssociation from '#lib/components/dots/DotAssociation.svelte';
	import { runLinks } from '#lib/run-links.js';
	import { webhooksApi, formatWebhookFire, type Webhook } from '#lib/webhooks.js';

	/**
	 * One webhook on `/webhooks`.
	 *
	 * The card answers the two questions the page exists for: is this hook
	 * live, and where is what it produced. The second one used to be a link on
	 * the last fire alone — `runLinks` picks between the one conversation and
	 * the list of them, by the same rule `/scheduled` uses.
	 */
	let { webhook, canWrite, onchanged, onnotice, onsecret } = $props<{
		webhook: Webhook;
		canWrite: boolean;
		onchanged: () => void | Promise<void>;
		onnotice: (message: string) => void;
		onsecret: (secret: string) => void;
	}>();

	// A webhook run stores the payload it fired with and the prompt that payload
	// was rendered into — neither is in the chat — so its history is worth
	// reaching even after a single fire.
	let links = $derived(runLinks(webhook, '/webhooks', { runsCarryMore: true, principalId: webhook.principal_id }));
	const scopeQuery = $derived(webhook.principal_id ? `?principal_id=${encodeURIComponent(webhook.principal_id)}` : '');
	let disposed = false;
	onDestroy(() => { disposed = true; });

	async function toggle() {
		if (!canWrite) return;
		try {
			await webhooksApi.toggle(webhook.id, !webhook.enabled, webhook.configuration_generation);
			if (disposed) return;
			await onchanged();
		} catch (caught) {
			if (!disposed) onnotice(String(caught));
		}
	}

	async function rotate() {
		if (!canWrite) return;
		if (!confirm(t('webhooks-rotate-confirm'))) return;
		try {
			const rotated = await webhooksApi.rotate(webhook.id, webhook.configuration_generation);
			if (!disposed) onsecret(rotated.secret);
		} catch (caught) {
			if (!disposed) onnotice(String(caught));
		}
	}

	async function remove() {
		if (!canWrite) return;
		if (!confirm(t('webhooks-delete-confirm'))) return;
		try {
			await webhooksApi.remove(webhook.id);
			if (disposed) return;
			await onchanged();
		} catch (caught) {
			if (!disposed) onnotice(String(caught));
		}
	}
</script>

<article class="card card-border">
	<div class="card-body gap-4">
		<div class="flex flex-col gap-3 sm:flex-row sm:items-start">
			<div class="min-w-0 flex-1">
				<div class="flex flex-wrap items-center gap-2">
					<h3 class="card-title text-base">{webhook.name}</h3>
					<span class="badge badge-sm {webhook.enabled ? 'badge-success' : 'badge-ghost'}">{webhook.enabled ? t('webhooks-badge-active') : t('webhooks-badge-paused')}</span>
					{#if webhook.reuse_conversation}<span class="badge badge-ghost badge-sm">{t('webhooks-badge-reuses-chat')}</span>{/if}
				</div>
				<DotAssociation binding={webhook} />
				{#if webhook.principal_id && webhook.dot}<p class="mt-2 text-sm">{webhook.dot.display || webhook.dot.name}</p>{/if}
				<p class="mt-1 line-clamp-2 text-sm text-base-content/70">{webhook.prompt}</p>
				<div class="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-xs text-base-content/60">
					{#if webhook.principal_id && webhook.agent_version}<span>{t('agents-test-source-published', { version: webhook.agent_version })}</span>{:else}<span class="font-mono">{webhook.model}</span>{/if}
					<span>·</span>
					<span>{t(webhook.synchronous ? 'webhooks-mode-sync' : 'webhooks-mode-async')}</span>
					<span>·</span>
					<span class="font-mono">/hooks/gwh_••••••••••••</span>
				</div>
				{#if webhook.last_fired_at}
					<p class="mt-1 text-xs {webhook.last_status === 'ok' ? 'text-success' : webhook.last_status === 'waiting' ? 'text-warning' : 'text-error'}">
						{t(webhook.last_status === 'ok' ? 'webhooks-last-success' : webhook.last_status === 'waiting' ? 'dot-background-last-waiting' : 'webhooks-last-failure', { when: formatWebhookFire(webhook.last_fired_at, locale.current) })}
					</p>
				{/if}
				{#if webhook.last_status === 'waiting'}<a class="link link-hover text-sm" href="/inbox">{t('inbox-heading')}</a>{/if}
				{#if webhook.last_error}<p class="mt-1 text-xs text-error">{webhook.last_error}</p>{/if}
			</div>
			{#if canWrite}<div class="flex flex-wrap gap-2">
				<a class="btn btn-sm" href={`/webhooks/${webhook.id}/edit${scopeQuery}`}>{t('webhooks-edit-title')}</a>
				<button class="btn btn-sm" type="button" onclick={toggle}>{webhook.enabled ? t('webhooks-pause-title') : t('webhooks-resume-title')}</button>
				<button class="btn btn-sm" type="button" onclick={rotate}>{t('webhooks-rotate-title')}</button>
				<button class="btn btn-outline btn-error btn-sm" type="button" onclick={remove}>{t('webhooks-delete-title')}</button>
			</div>{/if}
		</div>

		<!-- What this webhook produced. A hook that has never fired says so
		     rather than showing dead links. -->
		<div class="flex flex-wrap items-center gap-3 border-t border-base-300 pt-3 text-sm">
			{#if links.chat}
				<a class="link link-hover font-medium" href={links.chat}>{t('webhooks-open-chat')}</a>
				{#if links.runs}<a class="link link-hover text-base-content/70" href={links.runs + scopeQuery}>{t('webhooks-open-runs', { count: webhook.run_count })}</a>{/if}
			{:else if links.runs}
				{#if webhook.last_chat_deleted && webhook.chat_count <= 1}<span class="badge badge-sm badge-outline text-base-content/60">{t('webhooks-run-chat-deleted')}</span>{/if}
				<a class="link link-hover font-medium" href={links.runs + scopeQuery}>
					{webhook.chat_count > 0 ? t('webhooks-open-chats', { count: webhook.chat_count }) : t('webhooks-open-runs', { count: webhook.run_count })}
				</a>
			{:else}
				<span class="text-base-content/50">{t('webhooks-never-fired')}</span>
			{/if}
			{#if canWrite && webhook.has_payload}<a class="link link-hover text-base-content/70" href={`/webhooks/${webhook.id}/rerun${scopeQuery}`}>{t('webhooks-rerun-link')}</a>{/if}
		</div>
	</div>
</article>
