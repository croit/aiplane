<script lang="ts">
	import { adminDelete } from '$lib/admin-client';
	import { t } from '$lib/i18n.svelte';
	import { poolCoverage, type Backend, type Coverage, type Pool } from '$lib/upstreams';
	import BackendCard from './BackendCard.svelte';
	import PoolEditor from './PoolEditor.svelte';
	import EditModal from '$lib/components/EditModal.svelte';

	interface Props {
		pool: Pool;
		backends: Backend[];
		allBackends: Backend[];
		pools: Pool[];
		usage: Record<string, number[]>;
		coverage?: Coverage[];
		poolKinds: string[];
		groups: string[];
		poolStrategies: string[];
		onChanged: () => void | Promise<void>;
	}

	let { pool, backends, allBackends, pools, usage, coverage: suppliedCoverage, poolKinds, poolStrategies, groups, onChanged }: Props = $props();
	let deleting = $state(false);
	let editing = $state(false);
	let coverage = $derived(suppliedCoverage ?? poolCoverage(pool, allBackends));
	let missing = $derived(pool.backends.filter((name) => !allBackends.some((backend) => backend.name === name)));
	let poolBackends = $derived(backends.map((backend) => ({
		...backend,
		live: pool.live_backends?.[backend.name] ?? backend.live
	})));
	let problems = $derived.by(() => {
		const result: string[] = [];
		if (pool.backends.length === 0) result.push(t('upstreams-problem-no-backends'));
		const live = poolBackends.flatMap((backend) => backend.live ? [backend] : []);
		if (live.length && live.every((backend) => !backend.live?.enabled)) result.push(t('upstreams-problem-all-drained'));
		else if (live.length && live.every((backend) => !backend.live?.healthy || !backend.live?.enabled)) result.push(t('upstreams-problem-all-down'));
		const rejected = live.filter((backend) => backend.live?.auth_failed).map((backend) => backend.name);
		if (rejected.length) result.push(t('upstreams-problem-auth', { backends: rejected.join(', ') }));
		const modelless = live.filter((backend) => backend.live?.models.length === 0).map((backend) => backend.name);
		if (modelless.length) result.push(t('upstreams-problem-no-models', { backends: modelless.join(', ') }));
		const brokenAliases = live.flatMap((backend) => backend.aliases
			.filter(({ target }) => target ? !backend.live?.models.includes(target) : backend.live?.models.length !== 1)
			.map(({ alias }) => `${backend.name}:${alias}`));
		if (brokenAliases.length) result.push(t('upstreams-problem-broken-aliases', { aliases: brokenAliases.join(', ') }));
		const partial = coverage.filter(({ serving, total }) => serving > 0 && serving < total).map(({ name, serving, total }) => `${name} (${serving}/${total})`);
		if (partial.length) result.push(t('upstreams-problem-partial-coverage', { models: partial.join(', ') }));
		const unserved = coverage.filter(({ serving }) => serving === 0).map(({ name }) => name);
		if (unserved.length && live.length) result.push(t('upstreams-problem-unserved-allowlist', { models: unserved.join(', ') }));
		if (missing.length) result.push(t('upstreams-problem-missing-backends', { backends: missing.join(', ') }));
		return result;
	});

	async function remove() {
		if (!deleting) {
			deleting = true;
			return;
		}
		await adminDelete(`/api/v0/admin/pools/${encodeURIComponent(pool.name)}`);
		deleting = false;
		await onChanged();
	}
</script>

<article class="card card-border bg-base-100" data-testid={`upstream-pool-${pool.name}`}>
	<div class="card-body gap-3">
		<header class="flex flex-wrap items-center gap-2">
			<h2 class="card-title font-mono text-base">{pool.name}</h2>
			<span class="badge badge-secondary badge-sm">{pool.kind}</span>
			<span class="badge badge-ghost badge-sm font-mono">{pool.strategy}</span>
			{#if pool.fallback_offline}<span class="badge badge-warning badge-outline badge-sm font-mono" title={t('backends-fallback-offline-title')}>{t('backends-fallback-offline-badge', { model: pool.fallback_offline })}</span>{/if}
			<span class={`text-xs ${pool.compliance_gdpr ? 'text-success' : 'text-error'}`}>{pool.compliance_gdpr ? '✓' : '✗'} {t('upstreams-comp-gdpr')}</span>
			<span class={`text-xs ${pool.compliance_nda ? 'text-success' : 'text-error'}`}>{pool.compliance_nda ? '✓' : '✗'} {t('upstreams-comp-nda')}</span>
			<span class={`text-xs ${pool.enforce_limits ? 'text-success' : 'text-error'}`}>{pool.enforce_limits ? '✓' : '✗'} {t('upstreams-comp-limits')}</span>
			<span class="flex-1"></span>
			<button class="btn btn-ghost btn-xs" type="button" onclick={() => (editing = true)}>{t('upstreams-edit-pool')}</button>
			<button class="btn btn-ghost btn-xs text-error" type="button" onclick={() => void remove()}>{deleting ? t('upstreams-delete-confirm') : t('pools-delete-pool')}</button>
		</header>

		{#if problems.length}
			<div class="alert alert-warning items-start text-sm" role="alert">
				<div>
					<div class="font-medium">{t('upstreams-problems-heading')}</div>
					<ul class="mt-1 list-disc ps-5">
						{#each problems as problem}<li>{problem}</li>{/each}
					</ul>
				</div>
			</div>
		{/if}

		{#if backends.length}
			<div class="flex flex-col gap-2">
				{#each poolBackends as backend (backend.name)}
					<BackendCard {backend} currentPool={pool.name} {pools} backendNames={allBackends.map((candidate) => candidate.name)} usage={usage[backend.name] ?? []} onChanged={onChanged} />
				{/each}
			</div>
		{:else}
			<p class="text-sm text-base-content/60">{t('backends-pool-empty')}</p>
		{/if}

		<details class="collapse collapse-arrow rounded-box border border-base-300 bg-base-200/30">
			<summary class="collapse-title min-h-0 py-2 text-sm font-medium">{t('upstreams-coverage-heading')}</summary>
			<div class="collapse-content border-t border-base-300 pt-3">
				<p class="mb-2 text-xs text-base-content/60">{t('upstreams-coverage-hint')}</p>
				<div class="flex flex-wrap gap-1">
					{#each coverage as item (item.name)}
						<span class={`badge badge-sm font-mono ${item.serving === 0 ? 'badge-error' : item.serving < item.total ? 'badge-warning' : 'badge-success badge-outline'}`} title={item.serving === 0 ? t('upstreams-coverage-none-title') : item.serving < item.total ? t('upstreams-coverage-partial-title') : t('upstreams-coverage-full-title')}>
							{item.name} · {item.serving}/{item.total}
						</span>
					{/each}
				</div>
			</div>
		</details>

		<EditModal
			bind:open={editing}
			wide
			footer="none"
			title={t('upstreams-edit-pool')}
			description={pool.name}
			cancellabel={t('upstreams-cancel')}
		>
			<PoolEditor {pool} backends={allBackends} {poolKinds} {poolStrategies} {groups} existingNames={pools.map((candidate) => candidate.name)} sortOrder={pool.sort_order} onSaved={onChanged} onCancel={() => (editing = false)} />
		</EditModal>
	</div>
</article>
