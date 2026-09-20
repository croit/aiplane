<script lang="ts">
	import { onMount } from 'svelte';
	import { base } from '$app/paths';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { adminDelete, adminJson, adminPut } from '$lib/admin-client';
	import type { AdminModelsData } from '$lib/admin-models';
	import AdminModelEditor from '$lib/components/admin/AdminModelEditor.svelte';
	import { t } from '$lib/i18n.svelte';

	// `?model=` rather than a path segment: model names carry slashes
	// (`Qwen/Qwen3-35B`), and a query value survives them without depending on
	// how any proxy in front of the gateway treats an encoded separator.
	let requested = $derived(page.url.searchParams.get('model'));
	let data = $state<AdminModelsData | null>(null);
	let error = $state<string | null>(null);
	let model = $derived(data?.models.find((entry) => entry.name === requested) ?? null);
	let missing = $derived(data !== null && model === null);

	// Both actions hand the outcome back to the list through `?notice=`, so
	// the confirmation lands where the operator ends up rather than flashing
	// on a page that is about to unmount.
	async function save(body: Record<string, string>) {
		await adminPut('/api/v0/admin/models', body);
		await goto(`${base}/admin/models?tab=catalog&notice=saved&model=${encodeURIComponent(body.model_name)}`);
	}

	async function clear(name: string) {
		// Clearing drops every stored override for the model and cannot be
		// undone — keep the confirmation the inline editor had.
		if (!confirm(t('admin-clear-overrides-confirm', { model: name }))) return;
		await adminDelete(`/api/v0/admin/models/${encodeURIComponent(name)}`);
		await goto(`${base}/admin/models?tab=catalog&notice=cleared&model=${encodeURIComponent(name)}`);
	}

	onMount(async () => {
		try {
			data = await adminJson<AdminModelsData>('/api/v0/admin/models');
		} catch (caught) {
			error = String(caught);
		}
	});
</script>

<svelte:head><title>{t('admin-edit-model-page-title')}</title></svelte:head>

<div class="w-full max-w-4xl">
	<a class="link link-hover text-sm text-base-content/60" href="{base}/admin/models?tab=catalog">← {t('admin-heading')}</a>
	<h1 class="m-0 mt-2 break-all font-mono text-2xl font-bold">{requested}</h1>

	{#if error}<div class="alert alert-error mt-4 text-sm"><span>{error}</span></div>{/if}

	{#if missing}
		<div class="alert alert-warning mt-4 text-sm"><span>{t('admin-model-not-found')}</span></div>
	{:else if data && model}
		<div class="card mt-5 border border-base-300">
			<div class="card-body">
				<AdminModelEditor
					{model}
					currency={data.currency}
					allModels={data.all_models}
					onsave={save}
					onclear={clear}
				oncancel={() => goto(`${base}/admin/models?tab=catalog`)}
				/>
			</div>
		</div>
	{:else if !error}
		<div class="skeleton mt-5 h-96 w-full"></div>
	{/if}
</div>
