<script lang="ts">
	import { untrack } from 'svelte';
	import { t } from '$lib/i18n.svelte';
	import SearchableSelect from '$lib/components/SearchableSelect.svelte';
	import { isWildcardSelected, multiSelectOptions, normalizeSelection } from '$lib/multi-select';

	export interface AdminGroup {
		name: string;
		description: string;
		is_admin: boolean;
		is_default: boolean;
		oidc_values: string[];
		tools: string[];
		skills: string[];
	}

	let { group = null, toolIds = [], skillNames = [], onsave, ondelete }: {
		group?: AdminGroup | null;
		toolIds?: string[];
		skillNames?: string[];
		onsave: (group: AdminGroup) => Promise<void>;
		ondelete?: (name: string) => Promise<void>;
	} = $props();

	let name = $state(untrack(() => group?.name ?? ''));
	let description = $state(untrack(() => group?.description ?? ''));
	let isAdmin = $state(untrack(() => group?.is_admin ?? false));
	let isDefault = $state(untrack(() => group?.is_default ?? false));
	let oidcValues = $state(untrack(() => group?.oidc_values.join(', ') ?? ''));
	let tools = $state(untrack(() => normalizeSelection(group?.tools ?? [])));
	let skills = $state(untrack(() => normalizeSelection(group?.skills ?? [])));
	let saving = $state(false);
	let error = $state<string | null>(null);

	const asOptions = (ids: string[]) => ids.map((id) => ({ value: id, label: id }));
	let toolOptions = $derived(
		multiSelectOptions(asOptions(toolIds), {
			wildcardLabel: t('multi-select-wildcard-tools'),
			shadowLabel: t('multi-select-shadowed'),
			unknownLabel: t('multi-select-unknown')
		}, tools)
	);
	let skillOptions = $derived(
		multiSelectOptions(asOptions(skillNames), {
			wildcardLabel: t('multi-select-wildcard-skills'),
			shadowLabel: t('multi-select-shadowed'),
			unknownLabel: t('multi-select-unknown')
		}, skills)
	);
	const summaryFor = (wildcard: string) => ({
		empty: t('multi-select-none'),
		counted: (count: number) => t('multi-select-count', { count }),
		wildcard
	});
	// `*` on an admin group is what the setup wizard seeds and is meant there.
	// On any other group it silently widens with every release that adds a tool.
	let wildcardUnreviewed = $derived(!isAdmin && (isWildcardSelected(tools) || isWildcardSelected(skills)));

	function splitList(value: string) {
		return value.split(',').map((item) => item.trim()).filter(Boolean);
	}

	async function save() {
		saving = true;
		error = null;
		try {
			await onsave({
				name: name.trim(),
				description,
				is_admin: isAdmin,
				is_default: isDefault,
				oidc_values: splitList(oidcValues),
				tools,
				skills
			});
			if (!group) {
				name = '';
				description = '';
				isAdmin = false;
				isDefault = false;
				oidcValues = '';
				tools = [];
				skills = [];
			}
		} catch (caught) {
			error = String(caught);
		} finally {
			saving = false;
		}
	}
</script>

<article class="card border border-base-300 bg-base-100">
	<div class="card-body gap-3 p-4">
		<form class="flex flex-col gap-3" onsubmit={(event) => { event.preventDefault(); save(); }}>
			<h2 class="font-semibold">{group?.name ?? t('groups-new-heading')}</h2>
			{#if error}<div class="alert alert-error py-2 text-sm"><span>{error}</span></div>{/if}
			<div class="grid grid-cols-1 gap-x-4 gap-y-3 sm:grid-cols-2">
				<label class="flex flex-col gap-1">
					<span class="label-text text-xs">{t('groups-field-name')}</span>
					<input class="input input-bordered input-sm w-full font-mono {group ? 'bg-base-200' : ''}" bind:value={name} readonly={group !== null} required placeholder={group ? undefined : 'developers'} />
				</label>
				<label class="flex flex-col gap-1">
					<span class="label-text text-xs">{t('groups-field-description')}</span>
					<input class="input input-bordered input-sm w-full" bind:value={description} />
				</label>
			</div>
			<div class="flex flex-wrap gap-4">
				<label class="label cursor-pointer justify-start gap-2"><input type="checkbox" class="checkbox checkbox-sm" bind:checked={isAdmin} /><span>{t('groups-field-admin')}</span></label>
				<label class="label cursor-pointer justify-start gap-2"><input type="checkbox" class="checkbox checkbox-sm" bind:checked={isDefault} /><span>{t('groups-field-default')}</span></label>
			</div>
			<label class="flex flex-col gap-1">
				<span class="label-text text-xs">{t('groups-field-oidc')}</span>
				<input class="input input-bordered input-sm w-full" bind:value={oidcValues} list="group-oidc-values" placeholder={t('groups-oidc-values-placeholder')} />
				<span class="text-xs text-base-content/60">{t('groups-field-oidc-help')}</span>
			</label>
			<div class="flex flex-col gap-1">
				<span class="label-text text-xs">{t('groups-field-tools')}</span>
				<SearchableSelect multiple bind:values={tools} options={toolOptions} size="sm" ariaLabel={t('groups-field-tools')} summary={summaryFor(t('multi-select-wildcard-tools'))} />
			</div>
			<div class="flex flex-col gap-1">
				<span class="label-text text-xs">{t('groups-field-skills')}</span>
				<SearchableSelect multiple bind:values={skills} options={skillOptions} size="sm" ariaLabel={t('groups-field-skills')} summary={summaryFor(t('multi-select-wildcard-skills'))} />
			</div>
			{#if wildcardUnreviewed}
				<div class="alert alert-warning py-2 text-xs"><span>{t('multi-select-wildcard-warning')}</span></div>
			{/if}
			<div class="flex justify-end"><button type="submit" class="btn btn-primary btn-sm" disabled={saving || !name.trim()}>{t('groups-save')}</button></div>
		</form>
		{#if group && ondelete}
			<div class="flex justify-end">
				<button class="btn btn-ghost btn-sm text-error" onclick={() => ondelete?.(group.name)}>{t('groups-delete')}</button>
			</div>
		{/if}
	</div>
</article>
