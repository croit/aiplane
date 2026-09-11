<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson } from '$lib/admin-client';
	import WebhookForm from '$lib/components/webhooks/WebhookForm.svelte';
	import WebhookRow from '$lib/components/webhooks/WebhookRow.svelte';
	import WebhookSecretReveal from '$lib/components/webhooks/WebhookSecretReveal.svelte';
	import { t } from '$lib/i18n.svelte';
	import type { WebhooksData } from '$lib/webhooks';

	let data = $state<WebhooksData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let secret = $state<string | null>(null);
	async function refresh() {
		try { data = await adminJson<WebhooksData>('/api/v0/webhooks'); error = null; }
		catch (caught) { error = String(caught); }
	}
	function reveal(value: string) { secret = `${location.origin}/hooks/${value}`; }
	onMount(refresh);
</script>

<div class="mx-auto w-full max-w-5xl">
	<h1 class="mb-2 text-2xl font-bold">{t('webhooks-heading')}</h1>
	<p class="mb-6 text-sm text-base-content/60">{t('webhooks-intro')}</p>
	{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}
	{#if secret}<WebhookSecretReveal url={secret} />{/if}
	{#if data}
		<WebhookForm models={data.models} onsaved={refresh} onsecret={reveal} />
		<section class="card border border-base-300"><div class="card-body"><h2 class="card-title">{t('webhooks-list-heading')}</h2><ul class="flex flex-col divide-y divide-base-300">{#each data.webhooks as webhook (webhook.id)}<WebhookRow {webhook} onchanged={refresh} onnotice={(message) => (notice = message)} onsecret={reveal} />{:else}<li class="py-2 text-sm text-base-content/60">{t('webhooks-list-empty')}</li>{/each}</ul></div></section>
	{/if}
</div>
