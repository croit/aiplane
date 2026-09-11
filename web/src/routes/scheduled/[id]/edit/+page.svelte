<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { adminJson } from '$lib/admin-client';
	import ScheduleForm from '$lib/components/scheduled/ScheduleForm.svelte';
	import { t } from '$lib/i18n.svelte';
	import type { ScheduledAction, ScheduledData } from '$lib/scheduled';

	let data = $state<ScheduledData | null>(null);
	let action = $state<ScheduledAction | null>(null);
	let error = $state<string | null>(null);

	onMount(async () => {
		try {
			data = await adminJson<ScheduledData>('/api/v0/scheduled');
			action = data.actions.find((candidate) => candidate.id === page.params.id) ?? null;
			if (!action) error = t('scheduled-toast-not-found');
		} catch (caught) {
			error = String(caught);
		}
	});
</script>

<div class="w-full">
	<div class="mb-4 flex items-center gap-3">
		<a class="btn btn-ghost btn-sm" href="/scheduled">← {t('scheduled-back')}</a>
		<h1 class="text-2xl font-bold">{t('scheduled-edit-heading')}</h1>
	</div>
	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if data && action}
		<ScheduleForm {action} models={data.models} defaultTimezone={data.default_timezone} onsaved={() => goto('/scheduled')} oncancel={() => goto('/scheduled')} />
	{/if}
</div>
