<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { adminJson, adminPost } from '$lib/admin-client';
	import WebhookSubpageHeader from '$lib/components/webhooks/WebhookSubpageHeader.svelte';
	import { t } from '$lib/i18n.svelte';
	import type { Webhook, WebhookRun, WebhooksData } from '$lib/webhooks';
	let webhook = $state<Webhook | null>(null);
	let run = $state<WebhookRun | null>(null);
	let prompt = $state('');
	let error = $state<string | null>(null);
	let busy = $state(false);
	onMount(async () => {
		try {
			const data = await adminJson<WebhooksData>('/api/v0/webhooks');
			webhook = data.webhooks.find((candidate) => candidate.id === page.params.id) ?? null;
			if (!webhook) { error = t('webhooks-toast-not-found'); return; }
			const runs = (await adminJson<{ runs: WebhookRun[] }>(`/api/v0/webhooks/${page.params.id}/runs`)).runs;
			const runId = page.url.searchParams.get('run');
			run = (runId ? runs.find((candidate) => candidate.id === runId) : runs[0]) ?? null;
			prompt = webhook.prompt;
		} catch (caught) { error = String(caught); }
	});
	async function rerun() {
		if (!webhook || !run) return;
		busy = true; error = null;
		try {
			const result = await adminPost<{ session_id: string; status: string; error: string | null }>(`/api/v0/webhooks/${webhook.id}/rerun`, { prompt, run: run.id });
			if (result.status === 'ok') await goto(`/chat/${result.session_id}`);
			else error = t('webhooks-toast-rerun-failed', { status: result.status }) + (result.error ? `: ${result.error}` : '');
		} catch (caught) { error = String(caught); } finally { busy = false; }
	}
</script>

<div class="w-full">
	<WebhookSubpageHeader title={t('webhooks-rerun-heading')} intro={t('webhooks-rerun-intro')} />
	{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
	{#if webhook && run}<form class="card mb-6 border border-base-300" onsubmit={(event) => { event.preventDefault(); void rerun(); }}><div class="card-body gap-4"><label class="flex flex-col gap-1"><div class="label"><span class="label-text">{t('webhooks-rerun-payload-label')}</span></div><textarea class="textarea min-h-36 w-full font-mono text-xs" readonly value={run.payload} aria-label={t('webhooks-rerun-payload-label')}></textarea></label><label class="flex flex-col gap-1"><div class="label"><span class="label-text">{t('webhooks-prompt-label')}</span></div><textarea class="textarea min-h-28 w-full" bind:value={prompt} maxlength="8000" required aria-label={t('webhooks-prompt-label')}></textarea></label><div class="card-actions justify-end"><button class="btn btn-primary" type="submit" disabled={busy || !prompt.trim()}>{t('webhooks-rerun-submit')}</button></div></div></form>{:else if webhook}<div class="alert alert-info"><span>{t('webhooks-rerun-no-payload-notice')}</span></div>{/if}
</div>
