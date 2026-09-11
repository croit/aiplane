<script lang="ts">
	import { untrack } from 'svelte';
	import { adminPut } from '$lib/admin-client';
	import { t } from '$lib/i18n.svelte';
	import { parseVoices, splitLines, splitList, type Backend, type Pool } from '$lib/upstreams';

	interface Props {
		pool?: Pool | null;
		backends: Backend[];
		poolKinds: string[];
		poolStrategies: string[];
		existingNames: string[];
		sortOrder: number;
		onSaved: () => void | Promise<void>;
		onCancel?: () => void;
	}

	let { pool = null, backends, poolKinds, poolStrategies, existingNames, sortOrder, onSaved, onCancel }: Props = $props();
	let name = $state(untrack(() => pool?.name ?? ''));
	let kind = $state(untrack(() => pool?.kind ?? 'chat'));
	let strategy = $state(untrack(() => pool?.strategy ?? 'least_inflight'));
	let fallbackOffline = $state(untrack(() => pool?.fallback_offline ?? ''));
	let models = $state(untrack(() => pool?.models.join(', ') ?? ''));
	let allowedGroups = $state(untrack(() => pool?.allowed_groups.join(', ') ?? ''));
	let assigned = $state(untrack(() => pool?.backends.slice() ?? []));
	let complianceGdpr = $state(untrack(() => pool?.compliance_gdpr ?? true));
	let complianceNda = $state(untrack(() => pool?.compliance_nda ?? true));
	let enforceLimits = $state(untrack(() => pool?.enforce_limits ?? true));
	let voices = $state(untrack(() => pool?.voices.map(({ lang, voice }) => `${lang}=${voice}`).join('\n') ?? ''));
	let offerVoices = $state(untrack(() => pool?.offer_voices.join('\n') ?? ''));
	let overwrite = $state(untrack(() => pool !== null));
	let busy = $state(false);
	let error = $state<string | null>(null);
	let nameTaken = $derived(!pool && existingNames.includes(name.trim()));

	function setAssigned(backendName: string, checked: boolean) {
		assigned = checked
			? [...assigned, backendName]
			: assigned.filter((candidate) => candidate !== backendName);
	}

	async function save() {
		busy = true;
		error = null;
		try {
			await adminPut('/api/v0/admin/pools', {
				name,
				kind,
				strategy,
				fallback_offline: fallbackOffline.trim() || null,
				compliance_gdpr: complianceGdpr,
				compliance_nda: complianceNda,
				enforce_limits: enforceLimits,
				sort_order: pool?.sort_order ?? sortOrder,
				allowed_groups: splitList(allowedGroups),
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
			<input class="input input-bordered input-sm font-mono w-full" bind:value={name} readonly={pool !== null} required />
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('pools-field-kind')}</span>
			<select class="select select-bordered select-sm w-full" bind:value={kind}>
				{#each poolKinds as option (option)}<option value={option}>{option}</option>{/each}
			</select>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('pools-field-strategy')}</span>
			<select class="select select-bordered select-sm w-full font-mono" bind:value={strategy}>
				{#each poolStrategies as option (option)}<option value={option}>{option}</option>{/each}
			</select>
			<span class="text-xs text-base-content/50">{t('pools-field-strategy-hint')}</span>
		</label>
	</div>
	<label class="flex flex-col gap-1">
		<span class="text-xs text-base-content/70">{t('pools-field-fallback-offline')}</span>
		<input class="input input-bordered input-sm font-mono w-full" bind:value={fallbackOffline} placeholder={t('pools-field-fallback-offline-placeholder')} />
	</label>
	<label class="flex flex-col gap-1">
		<span class="text-xs text-base-content/70">{t('pools-field-models')}</span>
		<input class="input input-bordered input-sm font-mono w-full" bind:value={models} />
		<span class="text-xs text-base-content/50">{t('pools-field-models-hint')}</span>
	</label>
	<label class="flex flex-col gap-1">
		<span class="text-xs text-base-content/70">{t('pools-field-allowed-groups')}</span>
		<input class="input input-bordered input-sm font-mono w-full" bind:value={allowedGroups} />
		<span class="text-xs text-base-content/50">{t('pools-field-allowed-groups-hint')}</span>
	</label>
	{#if kind === 'speech'}
		<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
			<label class="flex flex-col gap-1">
				<span class="text-xs text-base-content/70">{t('pools-field-voices')}</span>
				<textarea class="textarea textarea-bordered textarea-sm font-mono w-full" rows="3" bind:value={voices}></textarea>
			</label>
			<label class="flex flex-col gap-1">
				<span class="text-xs text-base-content/70">{t('pools-field-offer-voices')}</span>
				<textarea class="textarea textarea-bordered textarea-sm font-mono w-full" rows="3" bind:value={offerVoices}></textarea>
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
		<label class="label gap-2"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={complianceGdpr} /><span>{t('pools-field-gdpr')}</span></label>
		<label class="label gap-2"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={complianceNda} /><span>{t('pools-field-nda')}</span></label>
		<label class="label gap-2"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={enforceLimits} /><span>{t('pools-field-enforce-limits')}</span></label>
	</div>
	<div class="flex justify-end gap-2">
		{#if onCancel}<button class="btn btn-ghost btn-sm" type="button" onclick={onCancel}>{t('upstreams-cancel')}</button>{/if}
		<button class="btn btn-primary btn-sm" type="submit" disabled={busy || !name.trim() || (nameTaken && !overwrite)}>
			{t('pools-save-pool')}
		</button>
	</div>
</form>
