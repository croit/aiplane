<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson } from '#lib/admin-client.js';
	import { t } from '#lib/i18n.svelte.js';
	import type { Topology } from '#lib/upstreams.js';
	import OpenRouterAccountPanel from '#lib/components/upstreams/OpenRouterAccountPanel.svelte';
	import OpenRouterEditor from '#lib/components/upstreams/OpenRouterEditor.svelte';
	import EditModal from '#lib/components/EditModal.svelte';

	interface View {
		backends: Topology['backends'];
		pools: Topology['pools'];
		pool_kinds: Topology['pool_kinds'];
	}

	// A whole account surface (key, kinds, catalogs, price policy) rather than
	// a footnote under the topology board: it lives on its own tab and reads
	// the topology it needs here.
	let data = $state<View | null>(null);
	let error = $state<string | null>(null);
	let addingOpenRouter = $state(false);

	async function refresh() {
		try {
			const topology = await adminJson<Topology>('/api/v0/admin/upstreams');
			data = {
				backends: topology.backends,
				pools: topology.pools,
				pool_kinds: topology.pool_kinds
			};
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	onMount(refresh);
</script>

{#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}

{#if data}
	{#if data.backends.some((backend) => backend.provider === 'openrouter')}
		<OpenRouterAccountPanel
			backends={data.backends}
			pools={data.pools}
			poolKinds={data.pool_kinds}
			onChanged={refresh}
		/>
	{:else}
		<section class="card card-border">
			<div class="card-body gap-3">
				<div>
					<h2 class="card-title text-base">{t('openrouter-heading')}</h2>
					<p class="text-sm text-base-content/70">{t('openrouter-description')}</p>
				</div>
				<div>
					<button class="btn btn-primary btn-sm" type="button" onclick={() => (addingOpenRouter = true)}>+ {t('openrouter-add-heading')}</button>
				</div>
			</div>
		</section>
	{/if}
{:else}
	<div class="flex justify-center py-10"><span class="loading loading-spinner"></span></div>
{/if}

<EditModal bind:open={addingOpenRouter} wide footer="none" title={t('openrouter-add-heading')} cancellabel={t('upstreams-cancel')}>
	<OpenRouterEditor
		pools={data?.pools ?? []}
		poolKinds={data?.pool_kinds ?? []}
		existingNames={(data?.backends ?? []).map((backend) => backend.name)}
		onSaved={() => {
			addingOpenRouter = false;
			return refresh();
		}}
		onCancel={() => (addingOpenRouter = false)}
	/>
</EditModal>
