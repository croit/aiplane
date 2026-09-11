<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson } from '$lib/admin-client';
	import ScheduleForm from '$lib/components/scheduled/ScheduleForm.svelte';
	import ScheduledActionRow from '$lib/components/scheduled/ScheduledActionRow.svelte';
	import { t } from '$lib/i18n.svelte';
	import type { ScheduledData } from '$lib/scheduled';

	let data = $state<ScheduledData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	async function refresh() {
		try {
			data = await adminJson<ScheduledData>('/api/v0/scheduled');
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	onMount(refresh);
</script>

<div class="mx-auto w-full max-w-5xl">
	<h1 class="mb-2 text-2xl font-bold">{t('scheduled-heading')}</h1>
	<p class="mb-6 text-sm text-base-content/60">{t('scheduled-intro')}</p>
	{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}
	{#if data}
		<ScheduleForm models={data.models} defaultTimezone={data.default_timezone} onsaved={refresh} />
		<section class="card border border-base-300">
			<div class="card-body">
				<h2 class="card-title">{t('scheduled-list-heading')}</h2>
				<ul class="flex flex-col divide-y divide-base-300">
					{#each data.actions as action (action.id)}
						<ScheduledActionRow {action} onchanged={refresh} onnotice={(message) => (notice = message)} />
					{:else}<li class="py-2 text-sm text-base-content/60">{t('scheduled-list-empty')}</li>{/each}
				</ul>
			</div>
		</section>
	{/if}
</div>
