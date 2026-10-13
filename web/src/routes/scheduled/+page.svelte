<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { page } from '$app/state';
	import { scheduledApi } from '#lib/scheduled.js';
	import ScheduledActionRow from '#lib/components/scheduled/ScheduledActionRow.svelte';
	import { t } from '#lib/i18n.svelte.js';
	import type { ScheduledData } from '#lib/scheduled.js';

	let data = $state<ScheduledData | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let disposed = false;
	onDestroy(() => { disposed = true; });

	// The create/edit form lives on its own route and reports back through the
	// URL, so its confirmation shows up on the list the user returns to.
	$effect(() => {
		const kind = page.url.searchParams.get('notice');
		if (kind === 'created') notice = t('scheduled-toast-created');
		else if (kind === 'saved') notice = t('scheduled-toast-saved');
	});

	async function refresh() {
		if (disposed) return;
		try {
			const loaded = await scheduledApi.list(page.url.searchParams.get('principal_id'));
			if (disposed) return;
			data = loaded;
			error = null;
		} catch (caught) {
			if (!disposed) error = String(caught);
		} finally {
			if (!disposed) loading = false;
		}
	}

	onMount(refresh);
</script>

<div class="w-full space-y-6">
	<header>
		<h1 class="text-2xl font-bold">{t('scheduled-heading')}</h1>
		<p class="mt-2 text-sm text-base-content/60">{t('scheduled-intro')}</p>
	</header>

	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-info"><span>{notice}</span></div>{/if}

	<section class="space-y-3">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<h2 class="text-xl font-semibold">{t('scheduled-list-heading')}</h2>
			{#if data?.can_write}<a class="btn btn-primary btn-sm" href={`/scheduled/new${data?.principal_id ? `?principal_id=${encodeURIComponent(data.principal_id)}` : ''}`}>{t('scheduled-create-heading')}</a>{/if}
		</div>
		{#if loading}
			<div class="skeleton h-28 w-full"></div>
		{:else if !data?.actions.length}
			<div class="card card-border"><div class="card-body text-sm text-base-content/60">{t('scheduled-list-empty')}</div></div>
		{:else}
			{#each data.actions as action (action.id)}
				<ScheduledActionRow {action} onchanged={refresh} onnotice={(message) => (notice = message)} />
			{/each}
		{/if}
	</section>
</div>
