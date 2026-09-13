<script lang="ts">
	import type { AdminSettingsField } from '$lib/admin-settings';
	import { settingsCatalogKey } from '$lib/admin-settings';
	import { t } from '$lib/i18n.svelte';
	import SearchableSelect from '$lib/components/SearchableSelect.svelte';

	let { field, draft, onchange, onclear }: { field: AdminSettingsField; draft: string; onchange: (key: string, value: string) => void; onclear: (key: string) => Promise<void> } = $props();
	let labelKey = $derived(settingsCatalogKey('settings-f-', field.key));
	let helpKey = $derived(`${labelKey}-help`);
	let label = $derived(t(labelKey) === labelKey ? (field.key.split('.').pop() ?? field.key) : t(labelKey));
	let help = $derived(t(helpKey) === helpKey ? '' : t(helpKey));
	let unavailableModel = $derived(field.kind === 'model' && draft !== '' && !field.models.includes(draft));
	let modelOptions = $derived([
		{ value: '', label: t('settings-model-automatic') },
		...field.models.map((model) => ({ value: model, label: model })),
		...(unavailableModel ? [{ value: draft, label: t('settings-model-unavailable', { model: draft }) }] : [])
	]);
</script>

<div class={field.span === 'full' ? 'flex flex-col gap-1 sm:col-span-2' : 'flex flex-col gap-1'}>
	<label class="flex flex-wrap items-baseline gap-2" for={field.key}><span class="text-sm font-medium">{label}</span>{#if field.restart}<span class="badge badge-warning badge-xs">{t('settings-restart-badge')}</span>{/if}</label>
	{#if field.kind === 'bool'}
		<input id={field.key} class="toggle toggle-sm" type="checkbox" checked={draft === 'true'} onchange={(event) => onchange(field.key, (event.currentTarget as HTMLInputElement).checked ? 'true' : 'false')} />
	{:else if field.kind === 'secret'}
		<div class="flex items-center gap-2"><input id={field.key} class="input input-bordered input-sm w-full" type="password" value={draft} placeholder={field.secret_set ? t('settings-secret-set') : t('settings-secret-unset')} autocomplete="new-password" oninput={(event) => onchange(field.key, (event.currentTarget as HTMLInputElement).value)} />{#if field.secret_set}<button type="button" class="btn btn-ghost btn-sm" onclick={() => onclear(field.key)}>{t('settings-secret-clear')}</button>{/if}</div>
	{:else if field.kind === 'choice'}
		<select id={field.key} class="select select-bordered select-sm w-full" value={draft} onchange={(event) => onchange(field.key, (event.currentTarget as HTMLSelectElement).value)}>
			{#each field.choices as choice (choice)}
				<option value={choice}>{t(`${labelKey}-opt-${choice}`) === `${labelKey}-opt-${choice}` ? choice : t(`${labelKey}-opt-${choice}`)}</option>
			{/each}
		</select>
	{:else if field.kind === 'model'}
		{#if field.models.length === 0 && draft === ''}<p class="m-0 text-sm italic text-base-content/60">{t('settings-model-none-configured')}</p>
		{:else}<SearchableSelect id={field.key} options={modelOptions} value={draft} onchange={(value) => onchange(field.key, value)} ariaLabel={label} size="sm" class="w-full" />{/if}
	{:else}
		<input id={field.key} class="input input-bordered input-sm w-full" type={field.kind === 'int' || field.kind === 'float' ? 'number' : 'text'} step={field.kind === 'float' ? 'any' : undefined} value={draft} oninput={(event) => onchange(field.key, (event.currentTarget as HTMLInputElement).value)} />
	{/if}
	<p class="m-0 break-all text-xs text-base-content/60"><code class="text-base-content/45">{field.key}</code>{#if help} · {help}{/if}</p>
</div>
