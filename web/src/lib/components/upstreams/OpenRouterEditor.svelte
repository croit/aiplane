<script lang="ts">
	import { tick } from 'svelte';
	import { adminPut } from '#lib/admin-client.js';
	import { t } from '#lib/i18n.svelte.js';
	import type { Pool } from '#lib/upstreams.js';

	interface Props {
		pools: Pool[];
		poolKinds: string[];
		existingNames: string[];
		onSaved: () => void | Promise<void>;
		onCancel?: () => void;
	}

	let { pools, poolKinds, existingNames, onSaved, onCancel }: Props = $props();
	let name = $state('openrouter');
	let apiKey = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);
	let errorAlert: HTMLDivElement | undefined = $state();
	let nameTaken = $derived(name.trim() !== 'openrouter' && existingNames.includes(name.trim()));
	let overwrite = $state(false);

	/**
	 * Every kind the account serves gets its own pool, created in the same
	 * save; a pool already existing under the default name is joined instead.
	 */
	async function save() {
		busy = true;
		error = null;
		try {
			const existing = new Set(pools.map((pool) => pool.name));
			const join: string[] = [];
			const create: { name: string; kind: string }[] = [];
			for (const kind of poolKinds.filter((candidate) => candidate !== 'ocr')) {
				const poolName = `openrouter-${kind}`;
				if (existing.has(poolName)) join.push(poolName);
				else create.push({ name: poolName, kind });
			}
			await adminPut('/api/v0/admin/backends', {
				name: name.trim(),
				provider: 'openrouter',
				base_url: 'https://openrouter.ai/api/v1',
				api_key: apiKey,
				weight: 1,
				max_inflight: 16,
				health_path: '/models',
				pools: join,
				new_pools: create,
				overwrite
			});
			await onSaved();
			onCancel?.();
		} catch (err) {
			error = String(err);
			await tick();
			errorAlert?.focus();
		} finally {
			busy = false;
		}
	}
</script>

<form class="flex flex-col gap-4" onsubmit={(event) => { event.preventDefault(); if (!busy) void save(); }}>
	{#if error}<div class="alert alert-error py-2 text-sm" role="alert" bind:this={errorAlert}>{error}</div>{/if}
	<p class="text-sm text-base-content/70">{t('openrouter-add-description')}</p>
	<div class="grid grid-cols-2 gap-4">
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('backends-field-name')}</span>
			<input class="input w-full" bind:value={name} required />
			{#if nameTaken && overwrite}<span class="text-xs text-error">{t('backends-name-taken')}</span>{/if}
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('backends-field-api-key')}</span>
			<input class="input font-mono w-full" type="password" autocomplete="off" bind:value={apiKey} placeholder={t('backends-field-api-key-placeholder')} required />
		</label>
	</div>
	{#if nameTaken}
		<label class="label gap-2"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={overwrite} /><span>{t('admin-overwrite-existing')}</span></label>
	{/if}
	<div class="flex items-center gap-2">
		<span class="flex-1"></span>
		{#if onCancel}<button class="btn btn-ghost btn-sm" type="button" onclick={onCancel}>{t('upstreams-cancel')}</button>{/if}
		<button class="btn btn-primary btn-sm" type="submit" disabled={busy || !name.trim() || !apiKey.trim() || (nameTaken && !overwrite)}>{t('openrouter-add-save')}</button>
	</div>
</form>
