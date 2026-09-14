<script lang="ts">
	import { onMount } from 'svelte';
	import { base } from '$app/paths';
	import { page } from '$app/state';
	import { adminJson } from '$lib/admin-client';
	import WebhookRow from '$lib/components/webhooks/WebhookRow.svelte';
	import WebhookSecretReveal from '$lib/components/webhooks/WebhookSecretReveal.svelte';
	import { t } from '$lib/i18n.svelte';
	import type { WebhooksData } from '$lib/webhooks';

	let data = $state<WebhooksData | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	// A rotated secret is revealed here, where the row that rotated it is. A
	// newly created one is revealed on the editor route instead — it must not
	// ride back in a query string.
	let secret = $state<string | null>(null);

	$effect(() => {
		const kind = page.url.searchParams.get('notice');
		if (kind === 'created') notice = t('webhooks-toast-created');
		else if (kind === 'saved') notice = t('webhooks-toast-saved');
	});

	async function refresh() {
		try {
			data = await adminJson<WebhooksData>('/api/v0/webhooks');
			error = null;
		} catch (caught) {
			error = String(caught);
		} finally {
			loading = false;
		}
	}

	function reveal(value: string) {
		secret = `${location.origin}/hooks/${value}`;
	}

	onMount(refresh);
</script>

<div class="w-full space-y-6">
	<header>
		<h1 class="text-2xl font-bold">{t('webhooks-heading')}</h1>
		<p class="mt-2 text-sm text-base-content/60">{t('webhooks-intro')}</p>
	</header>

	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-info"><span>{notice}</span></div>{/if}
	{#if secret}<WebhookSecretReveal url={secret} />{/if}

	<section class="space-y-3">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<h2 class="text-xl font-semibold">{t('webhooks-list-heading')}</h2>
			<a class="btn btn-primary btn-sm" href="{base}/webhooks/new">{t('webhooks-create-heading')}</a>
		</div>
		{#if loading}
			<div class="skeleton h-28 w-full"></div>
		{:else if !data?.webhooks.length}
			<div class="card card-border"><div class="card-body text-sm text-base-content/60">{t('webhooks-list-empty')}</div></div>
		{:else}
			{#each data.webhooks as webhook (webhook.id)}
				<WebhookRow {webhook} onchanged={refresh} onnotice={(message) => (notice = message)} onsecret={reveal} />
			{/each}
		{/if}
	</section>
</div>
