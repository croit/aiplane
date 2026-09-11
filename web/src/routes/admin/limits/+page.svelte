<script lang="ts">
	import { onMount } from 'svelte';
	import { adminDelete, adminJson, adminPost } from '$lib/admin-client';
	import type { AdminLimitsData, AdminLimitRule } from '$lib/admin-limits';
	import AdminLimitForm from '$lib/components/admin/AdminLimitForm.svelte';
	import AdminLimitsTable from '$lib/components/admin/AdminLimitsTable.svelte';
	import { t } from '$lib/i18n.svelte';

	let data = $state<AdminLimitsData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	async function refresh() {
		try { data = await adminJson<AdminLimitsData>('/api/v0/admin/limits'); error = null; }
		catch (caught) { error = String(caught); }
	}

	async function save(body: Record<string, string | number>, subject: string) {
		error = null;
		try { await adminPost('/api/v0/admin/limits', body); notice = t('limits-saved', { subject }); await refresh(); }
		catch (caught) { error = String(caught); }
	}

	async function remove(rule: AdminLimitRule) {
		if (!confirm(t('limits-delete-confirm'))) return;
		error = null;
		try { await adminDelete(`/api/v0/admin/limits/${encodeURIComponent(rule.id)}`); notice = t('limits-deleted'); await refresh(); }
		catch (caught) { error = String(caught); }
	}

	onMount(refresh);
</script>

<div class="mx-auto flex w-full max-w-5xl flex-col gap-4 px-4 pb-6 pt-14 sm:px-6 sm:pt-6">
	<header class="flex flex-col gap-1"><h1 class="m-0 text-2xl font-semibold">{t('limits-heading')}</h1><p class="m-0 text-sm text-base-content/70">{t('limits-intro')}</p></header>
	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-success"><span>{notice}</span></div>{/if}
	{#if data}<AdminLimitForm {data} onsave={save} /><AdminLimitsTable {data} onremove={remove} />
	{:else if !error}<div class="flex flex-col gap-4"><div class="skeleton h-64 w-full"></div><div class="skeleton h-40 w-full"></div></div>{/if}
</div>
