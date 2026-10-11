<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { scheduledApi } from '#lib/scheduled.js';
	import ScheduleForm from './ScheduleForm.svelte';
	import { t } from '#lib/i18n.svelte.js';
	import type { ScheduledAction, ScheduledData } from '#lib/scheduled.js';

	/**
	 * The schedule editor, on its own route.
	 *
	 * The create form used to sit permanently expanded above the list: a
	 * prompt box, a model picker, a five-mode schedule builder and two
	 * toggles, so the schedules a user came to look at started below the fold
	 * and the page only got longer as they added more. It gets a page now,
	 * and `/scheduled` gets an Add button.
	 *
	 * `id = null` is the create form; everything else is identical, so both
	 * routes render this.
	 */
	let { id = null }: { id?: string | null } = $props();

	let data = $state<ScheduledData | null>(null);
	let action = $state<ScheduledAction | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let missing = $state(false);
	let disposed = false;
	onDestroy(() => { disposed = true; });
	const listHref = $derived(`/scheduled${data?.principal_id || page.url.searchParams.get('principal_id') ? `?principal_id=${encodeURIComponent(data?.principal_id ?? page.url.searchParams.get('principal_id')!)}` : ''}`);

	async function done() {
		if (disposed) return;
		await goto(`/scheduled?notice=${id === null ? 'created' : 'saved'}${data?.principal_id ? `&principal_id=${encodeURIComponent(data.principal_id)}` : ''}`);
	}

	onMount(async () => {
		try {
			const loaded = await scheduledApi.list(page.url.searchParams.get('principal_id'));
			if (disposed) return;
			data = loaded;
			if (id !== null) {
				action = data.actions.find((candidate) => candidate.id === id) ?? null;
				missing = action === null;
			}
			error = null;
		} catch (caught) {
			if (!disposed) error = String(caught);
		} finally {
			if (!disposed) loading = false;
		}
	});
</script>

<div class="w-full max-w-4xl">
	<a class="link link-hover text-sm text-base-content/60" href={listHref}>← {t('scheduled-back')}</a>
	<h1 class="m-0 mt-2 text-2xl font-bold">
		{id === null ? t('scheduled-create-heading') : action ? t('scheduled-edit-named-heading', { name: action.name }) : t('scheduled-edit-heading')}
	</h1>

	{#if error}<div class="alert alert-error mt-4 text-sm"><span>{error}</span></div>{/if}

	{#if missing}
		<div class="alert alert-warning mt-4 text-sm"><span>{t('scheduled-toast-not-found')}</span></div>
	{:else if loading || !data}
		<div class="skeleton mt-5 h-96 w-full"></div>
	{:else}
		<div class="mt-5">
			<ScheduleForm
				{action}
				models={data.models}
				dots={data.dots}
				presetDotId={id === null ? page.url.searchParams.get('dot_id') : null}
				defaultTimezone={data.default_timezone}
				principalId={data.principal_id}
				versions={data.versions}
				canWrite={data.can_write}
				onsaved={done}
				oncancel={() => goto(listHref)}
			/>
		</div>
	{/if}
</div>
