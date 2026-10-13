<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import DotAssociation from '#lib/components/dots/DotAssociation.svelte';
	import { page } from '$app/state';
	import WebhookSubpageHeader from '#lib/components/webhooks/WebhookSubpageHeader.svelte';
	import { locale, t } from '#lib/i18n.svelte.js';
	import { webhooksApi, formatWebhookRun, type Webhook, type WebhookRun } from '#lib/webhooks.js';
	import { runLinks } from '#lib/run-links.js';
	let webhook = $state<Webhook | null>(null);
	let runs = $state<WebhookRun[]>([]);
	let error = $state<string | null>(null);
	let canWrite = $state(false);
	let disposed = false;
	onDestroy(() => { disposed = true; });
	const scopeQuery = $derived(webhook?.principal_id ? `principal_id=${encodeURIComponent(webhook.principal_id)}` : '');
	const backHref = $derived(`/webhooks${webhook?.principal_id || page.url.searchParams.get('principal_id') ? `?principal_id=${encodeURIComponent(webhook?.principal_id ?? page.url.searchParams.get('principal_id')!)}` : ''}`);
	onMount(async () => {
		try {
			const data = await webhooksApi.list(page.url.searchParams.get('principal_id'));
			if (disposed) return;
			canWrite = data.can_write;
			webhook = data.webhooks.find((candidate) => candidate.id === page.params.id) ?? null;
			if (!webhook) { error = t('webhooks-toast-not-found'); return; }
			const loaded = await webhooksApi.runs(page.params.id!);
			if (!disposed) runs = loaded.runs;
		} catch (caught) { if (!disposed) error = String(caught); }
	});
</script>

<div class="w-full">
	<WebhookSubpageHeader title={webhook ? t('webhooks-runs-heading', { name: webhook.name }) : t('webhooks-runs-link')} intro={t('webhooks-runs-intro')} {backHref} />
	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if webhook}<DotAssociation binding={webhook} />{#if webhook.principal_id && webhook.dot}<p class="mb-4 text-sm">{webhook.dot.display || webhook.dot.name}</p>{/if}<section class="card border border-base-300"><div class="card-body"><ul class="flex flex-col divide-y divide-base-300">{#each runs as run (run.id)}<li class="flex flex-col gap-2 py-3 sm:flex-row sm:items-start"><div class="min-w-0 flex-1"><div class="flex flex-wrap items-center gap-2"><strong class="text-sm">{formatWebhookRun(run.fired_at, locale.current)}</strong><span class="badge badge-sm {run.status === 'ok' ? 'badge-success' : run.status === 'error' ? 'badge-error' : run.status === 'waiting' ? 'badge-warning' : 'badge-ghost'}">{t(run.status === 'ok' ? 'webhooks-run-status-ok' : run.status === 'error' ? 'webhooks-run-status-error' : run.status === 'waiting' ? 'dot-background-waiting' : 'webhooks-run-status-pending')}</span><span class="badge badge-outline badge-sm">{t(run.source === 'rerun' ? 'webhooks-run-source-rerun' : 'webhooks-run-source-fire')}</span></div><p class="mt-0.5 truncate text-xs text-base-content/60">{run.prompt}</p>{#if run.agent_version}<p class="text-xs text-base-content/60">{t('agents-test-source-published', { version: run.agent_version })}</p>{/if}{#if run.error}<p class="mt-0.5 text-xs text-error">{run.error}</p>{/if}</div><div class="flex shrink-0 items-center gap-3 self-end text-sm sm:self-auto">{#if run.status === 'waiting'}<a class="link link-hover" href="/inbox">{t('inbox-heading')}</a>{/if}{#if run.chat_deleted}<span class="badge badge-sm badge-outline text-base-content/60">{t('webhooks-run-chat-deleted')}</span>{:else if run.session_id}<a class="link link-hover" href={runLinks({ id: run.id, last_session_id: run.session_id, run_count: 1, chat_count: 1 }, '/webhooks', { principalId: webhook.principal_id }).chat!}>{t('webhooks-run-open')}</a>{/if}{#if canWrite}<a class="link link-hover" href={`/webhooks/${webhook.id}/rerun?run=${run.id}${scopeQuery ? `&${scopeQuery}` : ''}`}>{t('webhooks-run-rerun')}</a>{/if}</div></li>{:else}<li class="py-2 text-sm text-base-content/60">{t('webhooks-runs-empty')}</li>{/each}</ul></div></section>{/if}
</div>
