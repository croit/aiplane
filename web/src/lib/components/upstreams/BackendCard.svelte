<script lang="ts">
	import { adminPost } from '$lib/admin-client';
	import { t, n } from '$lib/i18n.svelte';
	import { activityCounts, naturalSort, parallelismMismatch, type Backend, type Pool } from '$lib/upstreams';
	import BackendEditor from './BackendEditor.svelte';
	import EditModal from '$lib/components/EditModal.svelte';

	interface Props {
		backend: Backend;
		currentPool?: string | null;
		pools: Pool[];
		/** Every backend name, so the editor can refuse a rename onto one. */
		backendNames?: string[];
		usage: number[];
		onChanged: () => void | Promise<void>;
	}

	let { backend, currentPool = null, pools, backendNames = [], usage, onChanged }: Props = $props();
	// The server sorts these by byte so the status payload is stable; display
	// order is a separate, human question.
	let models = $derived(naturalSort(backend.live?.models ?? []));
	let withheld = $derived(naturalSort(backend.live?.withheld ?? []));
	let showInactive = $state(false);
	let editing = $state(false);
	let toggling = $state(false);
	let toggleError = $state<string | null>(null);
	let activity = $derived(activityCounts(usage));
	// The server said it runs fewer requests at once than we are configured to
	// send it. Not an error anywhere — Ollama queues rather than rejecting — so
	// the only place it can surface is here.
	let parallelWarning = $derived(
		parallelismMismatch(backend.live?.detected_max_parallel, backend.live?.max_inflight ?? backend.max_inflight)
	);
	let saturated = $derived(!!backend.live?.healthy && backend.live.inflight >= backend.live.max_inflight);
	let sparkline = $derived.by(() => {
		const maximum = Math.max(1, ...usage);
		return usage.map((value, index) => `${index * 8},${20 - (value / maximum) * 20}`).join(' ');
	});

	function status() {
		if (!backend.live) return { label: t('upstreams-backend-pending'), class: 'badge-ghost' };
		if (!backend.live.enabled) return { label: t('backends-status-drained'), class: 'badge-neutral' };
		if (!backend.live.healthy) return { label: t('backends-status-down'), class: 'badge-error' };
		if (saturated) return { label: t('backends-status-saturated'), class: 'badge-warning' };
		return { label: t('backends-status-up'), class: 'badge-success' };
	}

	function aliasTarget(alias: Backend['aliases'][number]): string | null {
		if (alias.target) return alias.target;
		return backend.live?.models.length === 1 ? backend.live.models[0] : null;
	}

	async function toggleServing(enabled: boolean) {
		toggling = true;
		toggleError = null;
		try {
			await adminPost(`/api/v0/admin/backends/${encodeURIComponent(backend.name)}/enabled`, { enabled });
			await onChanged();
		} catch (err) {
			toggleError = String(err);
		} finally {
			toggling = false;
		}
	}
</script>

<section class="rounded-box border border-base-300 bg-base-200/30" data-testid={`upstream-backend-${backend.name}`}>
	<div class="flex flex-col gap-2 px-3 py-2">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div class="flex min-w-0 items-center gap-2">
				<span class={`badge badge-sm ${status().class}`}>{status().label}</span>
				{#if backend.live?.auth_failed}<span class="badge badge-error badge-sm" title={t('backends-auth-failed-title')}>{t('backends-auth-failed')}</span>{/if}
				{#if backend.live && backend.live.models.length === 0}<span class="badge badge-warning badge-sm" title={t('backends-no-models-title')}>{t('backends-no-models')}</span>{/if}
				{#if backend.live && backend.live.profile !== 'generic'}<span class="badge badge-ghost badge-sm font-mono" title={[backend.live.detected_version, backend.live.detected_at].filter(Boolean).join(' · ') || undefined}>{t('backends-detect-profile', { profile: backend.live.profile })}</span>{/if}
				{#if parallelWarning !== null}<span class="badge badge-warning badge-sm">{t('backends-parallel-mismatch', { parallel: parallelWarning })}</span>{/if}
				{#if !backend.has_stored_key && backend.api_key_env}
					<span class={`badge badge-sm font-mono ${backend.api_key_env_set ? 'badge-ghost' : 'badge-error'}`}>
						{backend.api_key_env_set ? t('backends-key-env-badge', { var: backend.api_key_env }) : t('backends-key-env-unset-badge', { var: backend.api_key_env })}
					</span>
				{/if}
				<div class="min-w-0">
					<div class="break-all font-mono text-sm font-medium">{backend.name}</div>
					<div class="break-all font-mono text-xs text-base-content/60">{backend.base_url}</div>
				</div>
			</div>
			<div class="flex shrink-0 flex-col items-end gap-1">
				<div class="flex items-center gap-2">
					{#if backend.live}
						<label class="label cursor-pointer gap-2" title={t('backends-enabled-hint')}>
							<input class="toggle toggle-sm" type="checkbox" checked={backend.live.enabled} disabled={toggling} aria-label={t('backends-enabled-label')} onchange={(event) => void toggleServing(event.currentTarget.checked)} />
							<span class="text-sm">{t('backends-enabled-label')}</span>
						</label>
					{/if}
					<button type="button" class="btn btn-ghost btn-xs" onclick={() => (editing = true)}>{t('upstreams-edit-backend')}</button>
				</div>
				<div class="flex items-center gap-2">
					<span class="whitespace-nowrap text-xs tabular-nums text-base-content/60">{t('backends-inflight-label', { load: `${n(backend.live?.inflight ?? 0)} / ${n(backend.live?.max_inflight ?? backend.max_inflight)}` })}</span>
					<progress class={`progress w-24 ${saturated ? 'progress-warning' : 'progress-primary'}`} value={backend.live?.inflight ?? 0} max={backend.live?.max_inflight ?? backend.max_inflight}></progress>
				</div>
				<div class="flex items-center gap-2 text-base-content/50">
					<span class="whitespace-nowrap text-xs tabular-nums">{t('backends-activity-summary', { m15: n(activity.m15), m30: n(activity.m30), m60: n(activity.m60) })}</span>
					{#if usage.length > 1}<svg class="h-5 w-24 text-primary" viewBox="0 0 88 20" aria-hidden="true"><polyline fill="none" stroke="currentColor" stroke-width="1.5" points={sparkline}></polyline></svg>{/if}
				</div>
			</div>
		</div>
		{#if toggleError}<div class="alert alert-error py-2 text-sm" role="alert"><span>{toggleError}</span></div>{/if}
		{#if backend.live?.models.length || backend.live?.withheld.length || backend.aliases.length}
			<div class="flex flex-wrap items-center gap-1">
				{#each models as model (model)}<span class="badge badge-ghost badge-sm font-mono">{model}</span>{/each}
				{#if showInactive}
					{#each withheld as model (model)}<span class="badge badge-ghost badge-sm font-mono line-through opacity-50" title={t('upstreams-model-withheld-title')}>{model}</span>{/each}
					<button class="btn btn-ghost btn-xs" type="button" onclick={() => (showInactive = false)}>{t('upstreams-models-inactive-hide')}</button>
			{:else if backend.live?.withheld.length}
					<button class="badge badge-ghost badge-sm" type="button" title={t('upstreams-model-withheld-title')} onclick={() => (showInactive = true)}>{t('upstreams-models-inactive-pill', { count: n(backend.live.withheld.length) })}</button>
				{/if}
				{#if backend.aliases.length}<span class="text-xs text-base-content/60">{t('backends-aliases-label')}</span>{/if}
				{#each backend.aliases as alias (alias.alias)}
					{@const target = aliasTarget(alias)}
					<span class={`badge badge-sm font-mono ${target && backend.live?.models.includes(target) ? 'badge-outline' : 'badge-error'}`} title={target ? t('backends-alias-target-title', { target }) : t('backends-alias-disabled-title')}>
						{#if target}{alias.alias} <span aria-hidden="true">→</span> {target}{:else}{t('backends-alias-disabled-label', { name: alias.alias })}{/if}
					</span>
				{/each}
			</div>
		{/if}
	</div>
	<EditModal
		bind:open={editing}
		wide
		footer="none"
		title={t('upstreams-edit-backend')}
		description={backend.name}
		cancellabel={t('upstreams-cancel')}
	>
		<BackendEditor {backend} {currentPool} {pools} existingNames={backendNames} onSaved={onChanged} onCancel={() => (editing = false)} />
	</EditModal>
</section>
