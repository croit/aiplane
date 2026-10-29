<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { page } from '$app/state';
	import WebhookRow from '#lib/components/webhooks/WebhookRow.svelte';
	import WebhookSecretReveal from '#lib/components/webhooks/WebhookSecretReveal.svelte';
	import { t } from '#lib/i18n.svelte.js';
	import { webhooksApi, type WebhooksData } from '#lib/webhooks.js';

	let data = $state<WebhooksData | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	// A rotated secret is revealed here, where the row that rotated it is. A
	// newly created one is revealed on the editor route instead — it must not
	// ride back in a query string.
	let secret = $state<string | null>(null);
	let disposed = false;
	onDestroy(() => { disposed = true; });
	const scopeQuery = $derived(data?.principal_id ? `?principal_id=${encodeURIComponent(data.principal_id)}` : '');

	$effect(() => {
		const kind = page.url.searchParams.get('notice');
		if (kind === 'created') notice = t('webhooks-toast-created');
		else if (kind === 'saved') notice = t('webhooks-toast-saved');
	});

	async function refresh() {
		if (disposed) return;
		try {
			const loaded = await webhooksApi.list(page.url.searchParams.get('principal_id'));
			if (disposed) return;
			data = loaded;
			error = null;
		} catch (caught) {
			if (!disposed) error = String(caught);
		} finally {
			if (!disposed) loading = false;
		}
	}

	function reveal(value: string) {
		if (!disposed) secret = `${location.origin}/hooks/${value}`;
	}

	onMount(refresh);
</script>

<div class="w-full space-y-6">
	<header>
		<h1 class="text-2xl font-bold">{t('webhooks-heading')}</h1>
		<p class="mt-2 text-sm text-base-content/60">{t('webhooks-intro')}</p>
		{#if data?.principal_id && data.agents[0]}<p class="mt-2 text-sm">{data.agents[0].display || data.agents[0].name}</p>{/if}
	</header>

	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-info"><span>{notice}</span></div>{/if}
	{#if secret}<WebhookSecretReveal url={secret} />{/if}

	<section class="space-y-3">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<h2 class="text-xl font-semibold">{t('webhooks-list-heading')}</h2>
			{#if data?.can_write && !error}<a class="btn btn-primary btn-sm" href={`/webhooks/new${scopeQuery}`}>{t('webhooks-create-heading')}</a>{/if}
		</div>
		{#if loading}
			<div class="skeleton h-28 w-full"></div>
		{:else if !data?.webhooks.length}
			<div class="card card-border"><div class="card-body text-sm text-base-content/60">{t('webhooks-list-empty')}</div></div>
		{:else}
			{#each data.webhooks as webhook (webhook.id)}
				<WebhookRow {webhook} canWrite={data.can_write && !error} onchanged={refresh} onnotice={(message) => (notice = message)} onsecret={reveal} />
			{/each}
		{/if}
	</section>
</div>
