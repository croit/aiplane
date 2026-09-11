<script lang="ts">
	import { untrack } from 'svelte';
	import { t } from '$lib/i18n.svelte';

	export interface AdminGroup {
		name: string;
		description: string;
		is_admin: boolean;
		is_default: boolean;
		oidc_values: string[];
		tools: string[];
		skills: string[];
	}

	let { group = null, onsave, ondelete }: {
		group?: AdminGroup | null;
		onsave: (group: AdminGroup) => Promise<void>;
		ondelete?: (name: string) => Promise<void>;
	} = $props();

	let name = $state(untrack(() => group?.name ?? ''));
	let description = $state(untrack(() => group?.description ?? ''));
	let isAdmin = $state(untrack(() => group?.is_admin ?? false));
	let isDefault = $state(untrack(() => group?.is_default ?? false));
	let oidcValues = $state(untrack(() => group?.oidc_values.join(', ') ?? ''));
	let tools = $state(untrack(() => group?.tools.join(', ') ?? ''));
	let skills = $state(untrack(() => group?.skills.join(', ') ?? ''));
	let saving = $state(false);
	let error = $state<string | null>(null);

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
				tools: splitList(tools),
				skills: splitList(skills)
			});
			if (!group) {
				name = '';
				description = '';
				isAdmin = false;
				isDefault = false;
				oidcValues = '';
				tools = '';
				skills = '';
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
			<label class="flex flex-col gap-1">
				<span class="label-text text-xs">{t('groups-field-tools')}</span>
				<input class="input input-bordered input-sm w-full" bind:value={tools} list="group-tool-ids" placeholder="*" />
			</label>
			<label class="flex flex-col gap-1">
				<span class="label-text text-xs">{t('groups-field-skills')}</span>
				<input class="input input-bordered input-sm w-full" bind:value={skills} list="group-skill-names" placeholder="*" />
			</label>
			<div class="flex justify-end"><button type="submit" class="btn btn-primary btn-sm" disabled={saving || !name.trim()}>{t('groups-save')}</button></div>
		</form>
		{#if group && ondelete}
			<div class="flex justify-end">
				<button class="btn btn-ghost btn-sm text-error" onclick={() => ondelete?.(group.name)}>{t('groups-delete')}</button>
			</div>
		{/if}
	</div>
</article>
