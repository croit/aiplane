<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPut } from '#lib/admin-client.js';
	import { t } from '#lib/i18n.svelte.js';
	import type { Topology } from '#lib/upstreams.js';
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';

	interface FallbacksView {
		fallbacks: Record<string, string>;
		fallback_kinds: string[];
		all_models: string[];
	}

	// Per kind, not per pool: the substitute a request gets when it names a
	// model no pool knows. Its own tab because it is a routing decision, not
	// a property of any backend.
	let data = $state<FallbacksView | null>(null);
	let error = $state<string | null>(null);
	let savedKind = $state<string | null>(null);
	let saving = $state(false);
	let savedTimer: ReturnType<typeof setTimeout> | null = null;

	async function refresh() {
		try {
			const topology = await adminJson<Topology>('/api/v0/admin/upstreams');
			data = {
				fallbacks: topology.fallbacks,
				fallback_kinds: topology.fallback_kinds,
				all_models: topology.all_models ?? []
			};
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	async function save(kind: string, model: string) {
		if (saving) return;
		saving = true;
		savedKind = null;
		try {
			await adminPut('/api/v0/admin/upstreams/fallback', { kind, model });
			if (data) data = { ...data, fallbacks: { ...data.fallbacks, [kind]: model } };
			savedKind = kind;
			if (savedTimer) clearTimeout(savedTimer);
			savedTimer = setTimeout(() => (savedKind = null), 4000);
		} catch (caught) {
			error = String(caught);
		} finally {
			saving = false;
		}
	}

	onMount(refresh);
</script>

{#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}

<section class="card card-border">
	<div class="card-body gap-3">
		<h2 class="card-title text-base">{t('pools-fallbacks-heading')}</h2>
		<p class="text-sm text-base-content/70">{t('pools-fallbacks-description')}</p>
		{#if data}
			<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
				{#each data.fallback_kinds as kind (kind)}
					<label class="flex flex-col gap-1">
						<span class="font-mono text-xs text-base-content/70">{kind}</span>
						<SearchableSelect
							options={[{ value: '', label: t('admin-cap-no-fallback') }, ...data.all_models.map((model) => ({ value: model, label: model }))]}
							value={data.fallbacks[kind] ?? ''}
							onchange={(value) => void save(kind, value)}
							ariaLabel={kind}
							size="sm"
							class="w-full"
						/>
						{#if savedKind === kind}<span class="text-xs text-success" role="status">{t('upstreams-saved')}</span>{/if}
					</label>
				{/each}
			</div>
		{/if}
	</div>
</section>
