<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { adminJson } from '$lib/admin-client';
	import WebhookSubpageHeader from '$lib/components/webhooks/WebhookSubpageHeader.svelte';
	import { locale, t } from '$lib/i18n.svelte';
	import { formatWebhookRun, type Webhook, type WebhookRun, type WebhooksData } from '$lib/webhooks';
	let webhook = $state<Webhook | null>(null);
	let runs = $state<WebhookRun[]>([]);
	let error = $state<string | null>(null);
	onMount(async () => {
		try {
			const data = await adminJson<WebhooksData>('/api/v0/webhooks');
			webhook = data.webhooks.find((candidate) => candidate.id === page.params.id) ?? null;
			if (!webhook) { error = t('webhooks-toast-not-found'); return; }
			runs = (await adminJson<{ runs: WebhookRun[] }>(`/api/v0/webhooks/${page.params.id}/runs`)).runs;
		} catch (caught) { error = String(caught); }
	});
</script>

<div class="w-full">
	<WebhookSubpageHeader title={webhook ? t('webhooks-runs-heading', { name: webhook.name }) : t('webhooks-runs-link')} intro={t('webhooks-runs-intro')} />
	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if webhook}<section class="card border border-base-300"><div class="card-body"><ul class="flex flex-col divide-y divide-base-300">{#each runs as run (run.id)}<li class="flex flex-col gap-2 py-3 sm:flex-row sm:items-start"><div class="min-w-0 flex-1"><div class="flex flex-wrap items-center gap-2"><strong class="text-sm">{formatWebhookRun(run.fired_at, locale.current)}</strong><span class="badge badge-sm {run.status === 'ok' ? 'badge-success' : run.status === 'error' ? 'badge-error' : 'badge-ghost'}">{t(run.status === 'ok' ? 'webhooks-run-status-ok' : run.status === 'error' ? 'webhooks-run-status-error' : 'webhooks-run-status-pending')}</span><span class="badge badge-outline badge-sm">{t(run.source === 'rerun' ? 'webhooks-run-source-rerun' : 'webhooks-run-source-fire')}</span></div><p class="mt-0.5 truncate text-xs text-base-content/60">{run.prompt}</p>{#if run.error}<p class="mt-0.5 text-xs text-error">{run.error}</p>{/if}</div><div class="flex shrink-0 items-center gap-3 self-end text-sm sm:self-auto">{#if run.session_id}<a class="link link-hover" href={`/chat/${run.session_id}`}>{t('webhooks-run-open')}</a>{/if}<a class="link link-hover" href={`/webhooks/${webhook.id}/rerun?run=${run.id}`}>{t('webhooks-run-rerun')}</a></div></li>{:else}<li class="py-2 text-sm text-base-content/60">{t('webhooks-runs-empty')}</li>{/each}</ul></div></section>{/if}
</div>
