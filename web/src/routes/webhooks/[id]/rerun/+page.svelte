<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { ApiError } from '#lib/api.js';
	import DotAssociation from '#lib/components/dots/DotAssociation.svelte';
	import WebhookSubpageHeader from '#lib/components/webhooks/WebhookSubpageHeader.svelte';
	import { t } from '#lib/i18n.svelte.js';
	import { webhooksApi, type Webhook, type WebhookRun } from '#lib/webhooks.js';
	import { runLinks } from '#lib/run-links.js';
	let webhook = $state<Webhook | null>(null);
	let run = $state<WebhookRun | null>(null);
	let prompt = $state('');
	let error = $state<string | null>(null);
	let busy = $state(false);
	let canWrite = $state(false);
	let disposed = false;
	onDestroy(() => { disposed = true; });
	const backHref = $derived(`/webhooks${webhook?.principal_id || page.url.searchParams.get('principal_id') ? `?principal_id=${encodeURIComponent(webhook?.principal_id ?? page.url.searchParams.get('principal_id')!)}` : ''}`);
	onMount(async () => {
		try {
			const data = await webhooksApi.list(page.url.searchParams.get('principal_id'));
			if (disposed) return;
			canWrite = data.can_write;
			webhook = data.webhooks.find((candidate) => candidate.id === page.params.id) ?? null;
			if (!webhook) { error = t('webhooks-toast-not-found'); return; }
			const runs = (await webhooksApi.runs(page.params.id!)).runs;
			if (disposed) return;
			const runId = page.url.searchParams.get('run');
			run = (runId ? runs.find((candidate) => candidate.id === runId) : runs[0]) ?? null;
			prompt = webhook.prompt;
		} catch (caught) { if (!disposed) error = String(caught); }
	});
	async function rerun() {
		if (disposed || !webhook || !run || !canWrite || busy) return;
		busy = true; error = null;
		try {
			const result = await webhooksApi.rerun(webhook.id, { prompt, run: run.id, expected_configuration_generation: webhook.configuration_generation });
			if (disposed) return;
			await goto(runLinks({ id: run.id, last_session_id: result.session_id, run_count: 1, chat_count: 1 }, '/webhooks', { principalId: webhook.principal_id }).chat!);
		} catch (caught) {
			if (disposed) return;
			if (caught instanceof ApiError && caught.code === 'run_failed') {
				const status = (JSON.parse(caught.detail ?? '{}') as { error?: { status?: string } }).error?.status ?? '';
				error = t('webhooks-toast-rerun-failed', { status }) + (caught.serverMessage ? `: ${caught.serverMessage}` : '');
			} else error = caught instanceof ApiError ? (caught.serverMessage ?? caught.message) : String(caught);
		} finally { if (!disposed) busy = false; }
	}
</script>

<div class="w-full">
	<WebhookSubpageHeader title={t('webhooks-rerun-heading')} intro={t('webhooks-rerun-intro')} {backHref} />
	{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
	{#if webhook}<DotAssociation binding={webhook} />{/if}
	{#if webhook?.principal_id && webhook.dot}<p class="mb-4 text-sm">{webhook.dot.display || webhook.dot.name}</p>{/if}
	{#if webhook && run}<form class="card mb-6 border border-base-300" onsubmit={(event) => { event.preventDefault(); void rerun(); }}><div class="card-body gap-4"><label class="flex flex-col gap-1"><div class="label"><span class="label-text">{t('webhooks-rerun-payload-label')}</span></div><textarea class="textarea min-h-36 w-full font-mono text-xs" readonly value={run.payload} aria-label={t('webhooks-rerun-payload-label')}></textarea></label><label class="flex flex-col gap-1"><div class="label"><span class="label-text">{t('webhooks-prompt-label')}</span></div><textarea class="textarea min-h-28 w-full" bind:value={prompt} maxlength="8000" required aria-label={t('webhooks-prompt-label')}></textarea></label><div class="card-actions justify-end"><button class="btn btn-primary" type="submit" disabled={!canWrite || busy || !prompt.trim()}>{t('webhooks-rerun-submit')}</button></div></div></form>{:else if webhook}<div class="alert alert-info"><span>{t('webhooks-rerun-no-payload-notice')}</span></div>{/if}
</div>
