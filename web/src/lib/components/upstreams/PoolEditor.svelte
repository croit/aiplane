<script lang="ts">
	import { untrack } from 'svelte';
	import { adminPost, adminPut } from '#lib/admin-client.js';
	import { t } from '#lib/i18n.svelte.js';
	import { parseVoices, splitLines, splitList, type Backend, type Pool } from '#lib/upstreams.js';
	import GroupSelect from '#lib/components/GroupSelect.svelte';
	import { normalizeSelection } from '#lib/multi-select.js';

	interface Props {
		pool?: Pool | null;
		backends: Backend[];
		knownModels?: string[];
		poolKinds: string[];
		poolStrategies: string[];
		groups: string[];
		existingNames: string[];
		sortOrder: number;
		onSaved: () => void | Promise<void>;
		onDelete?: () => void | Promise<void>;
		onCancel?: () => void;
	}

	let { pool = null, backends, knownModels = [], poolKinds, poolStrategies, groups, existingNames, sortOrder, onSaved, onDelete, onCancel }: Props = $props();
	let name = $state(untrack(() => pool?.name ?? ''));
	let kind = $state(untrack(() => pool?.kind ?? 'chat'));
	let strategy = $state(untrack(() => pool?.strategy ?? 'least_inflight'));
	let fallbackOffline = $state(untrack(() => pool?.fallback_offline ?? ''));
	let models = $state(untrack(() => pool?.models.join(', ') ?? ''));
	let allowedGroups = $state(untrack(() => normalizeSelection(pool?.allowed_groups ?? [])));
	let assigned = $state(untrack(() => pool?.backends.slice() ?? []));
	let enforceLimits = $state(untrack(() => pool?.enforce_limits ?? true));
	let voices = $state(untrack(() => pool?.voices.map(({ lang, voice }) => `${lang}=${voice}`).join('\n') ?? ''));
	let offerVoices = $state(untrack(() => pool?.offer_voices.join('\n') ?? ''));
	let overwrite = $state(untrack(() => pool !== null));
	let busy = $state(false);
	let error = $state<string | null>(null);
	// A new pool, or an existing one being renamed onto a name in use.
	let nameTaken = $derived(name.trim() !== pool?.name && existingNames.includes(name.trim()));
	let pinned = $derived(splitList(models));

	function toggleModel(model: string, checked: boolean) {
		const current = splitList(models);
		const next = checked
			? current.includes(model) ? current : [...current, model]
			: current.filter((candidate) => candidate !== model);
		models = next.join(', ');
	}

	async function remove() {
		if (!pool || !onDelete) return;
		if (!confirm(t('pools-delete-confirm', { name: pool.name }))) return;
		busy = true;
		error = null;
		try {
			await onDelete();
		} catch (err) {
			error = String(err);
		} finally {
			busy = false;
		}
	}

	function setAssigned(backendName: string, checked: boolean) {
		assigned = checked
			? [...assigned, backendName]
			: assigned.filter((candidate) => candidate !== backendName);
	}

	async function save() {
		busy = true;
		error = null;
		try {
			// A changed name is a *rename*, not a save under a new name: the
			// name is the primary key, so an ordinary save would leave the
			// original row behind and start a second, empty pool. Move the
			// identity first, then write the fields under it.
			if (pool && name.trim() !== pool.name) {
				await adminPost(`/api/v0/admin/pools/${encodeURIComponent(pool.name)}/rename`, { name: name.trim() });
			}
			await adminPut('/api/v0/admin/pools', {
				name,
				kind,
				strategy,
				fallback_offline: fallbackOffline.trim() || null,
				enforce_limits: enforceLimits,
				sort_order: pool?.sort_order ?? sortOrder,
				allowed_groups: allowedGroups,
				backends: assigned,
				models: splitList(models),
				voices: parseVoices(voices),
				offer_voices: splitLines(offerVoices),
				overwrite
			});
			await onSaved();
			onCancel?.();
		} catch (err) {
			error = String(err);
		} finally {
			busy = false;
		}
	}
</script>

<form class="flex flex-col gap-3" onsubmit={(event) => { event.preventDefault(); void save(); }}>
	{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}
	{#if nameTaken}
		<div class="alert alert-warning text-sm" role="alert">
			<span>{t('pools-name-taken')}</span>
			<label class="label gap-2">
				<input class="checkbox checkbox-sm" type="checkbox" bind:checked={overwrite} />
				<span>{t('admin-overwrite-existing')}</span>
			</label>
		</div>
	{/if}
	<div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('pools-field-name')}</span>
			<input class="input font-mono w-full" bind:value={name} required />
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('pools-field-kind')}</span>
			<select class="select w-full" bind:value={kind}>
				{#each poolKinds as option (option)}<option value={option}>{option}</option>{/each}
			</select>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('pools-field-strategy')}</span>
			<select class="select w-full font-mono" bind:value={strategy}>
				{#each poolStrategies as option (option)}<option value={option}>{option}</option>{/each}
			</select>
			<span class="text-xs text-base-content/50">{t('pools-field-strategy-hint')}</span>
		</label>
	</div>
	<label class="flex flex-col gap-1">
		<span class="text-xs text-base-content/70">{t('pools-field-fallback-offline')}</span>
		<input class="input font-mono w-full" bind:value={fallbackOffline} placeholder={t('pools-field-fallback-offline-placeholder')} />
	</label>
	<div class="flex flex-col gap-2">
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('pools-field-models')}</span>
			<input class="input font-mono w-full" bind:value={models} />
			<span class="text-xs text-base-content/50">{t('pools-field-models-hint')}</span>
		</label>
		{#if knownModels.length}
			<div class="flex flex-col gap-1">
				<span class="text-xs text-base-content/60">{t('pools-field-models-chips')}</span>
				<div class="flex flex-wrap gap-1.5">
					{#each knownModels as model (model)}
						<label class="chip-choice" title={model}>
							<input class="checkbox checkbox-xs" type="checkbox" checked={pinned.includes(model)} onchange={(event) => toggleModel(model, event.currentTarget.checked)} />
							<span class="font-mono">{model}</span>
						</label>
					{/each}
				</div>
			</div>
		{/if}
	</div>
	<div class="flex flex-col gap-1">
		<span class="text-xs text-base-content/70">{t('pools-field-allowed-groups')}</span>
		<GroupSelect {groups} bind:values={allowedGroups} label={t('pools-field-allowed-groups')} />
		<span class="text-xs text-base-content/50">{t('pools-field-allowed-groups-hint')}</span>
	</div>
	{#if kind === 'speech'}
		<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
			<label class="flex flex-col gap-1">
				<span class="text-xs text-base-content/70">{t('pools-field-voices')}</span>
				<textarea class="textarea font-mono w-full" rows="3" bind:value={voices}></textarea>
			</label>
			<label class="flex flex-col gap-1">
				<span class="text-xs text-base-content/70">{t('pools-field-offer-voices')}</span>
				<textarea class="textarea font-mono w-full" rows="3" bind:value={offerVoices}></textarea>
			</label>
		</div>
	{/if}
	<fieldset class="flex flex-col gap-1">
		<legend class="text-xs text-base-content/70">{t('pools-field-backends')}</legend>
		{#if backends.length === 0}
			<span class="text-xs italic text-base-content/50">{t('pools-no-backends')}</span>
		{:else}
			<div class="flex flex-wrap gap-x-4 gap-y-1">
				{#each backends as backend (backend.name)}
					<label class="label gap-2">
						<input class="checkbox checkbox-sm" type="checkbox" checked={assigned.includes(backend.name)} onchange={(event) => setAssigned(backend.name, event.currentTarget.checked)} />
						<span class="font-mono text-sm">{backend.name}</span>
					</label>
				{/each}
			</div>
		{/if}
	</fieldset>
	<div class="flex flex-wrap gap-4">
		<label class="label gap-2"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={enforceLimits} /><span>{t('pools-field-enforce-limits')}</span></label>
	</div>
	<div class="sticky bottom-0 z-10 flex flex-wrap items-center justify-end gap-2 border-t border-base-300 bg-base-100 py-3">
		{#if pool && onDelete}
			<button class="btn btn-ghost btn-sm text-error me-auto" type="button" disabled={busy} onclick={() => void remove()}>{t('pools-delete-pool')}</button>
		{/if}
		{#if onCancel}<button class="btn btn-ghost btn-sm" type="button" onclick={onCancel}>{t('upstreams-cancel')}</button>{/if}
		<button class="btn btn-primary btn-sm" type="submit" disabled={busy || !name.trim() || (nameTaken && !overwrite)}>
			{t('pools-save-pool')}
		</button>
	</div>
</form>

<style>
	.chip-choice {
		display: inline-flex;
		align-items: center;
		gap: 0.375rem;
		max-width: 100%;
		border: 1px solid var(--color-base-300);
		border-radius: calc(var(--radius-box) / 3);
		padding: 0.125rem 0.5rem;
		font-size: 0.6875rem;
		cursor: pointer;
	}
	.chip-choice:hover {
		background-color: color-mix(in oklab, var(--color-base-content) 5%, transparent);
	}
	.chip-choice span {
		overflow-wrap: anywhere;
	}
</style>
