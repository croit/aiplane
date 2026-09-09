<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPut, adminDelete } from '$lib/admin-client';
	import { t } from '$lib/i18n.svelte';

	interface Group {
		name: string;
		description: string;
		is_admin: boolean;
		is_default: boolean;
		oidc_values: string[];
		tools: string[];
		skills: string[];
	}
	interface GroupsData {
		groups: Group[];
		observed_oidc_values: string[];
		tool_ids: string[];
		skill_names: string[];
	}

	let data = $state<GroupsData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let editing = $state<string | null>(null); // null = closed, "" = new, name = edit

	// Form state (bound inputs).
	let fname = $state('');
	let fdescription = $state('');
	let fisAdmin = $state(false);
	let fisDefault = $state(false);
	let foidc = $state('');
	let ftools = $state('');
	let fskills = $state('');

	async function refresh() {
		try {
			data = await adminJson<GroupsData>('/api/v0/admin/groups');
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	function openNew() {
		editing = '';
		fname = '';
		fdescription = '';
		fisAdmin = false;
		fisDefault = false;
		foidc = '';
		ftools = '';
		fskills = '';
	}

	function openEdit(g: Group) {
		editing = g.name;
		fname = g.name;
		fdescription = g.description;
		fisAdmin = g.is_admin;
		fisDefault = g.is_default;
		foidc = g.oidc_values.join(', ');
		ftools = g.tools.join(', ');
		fskills = g.skills.join(', ');
	}

	async function save() {
		notice = null;
		try {
			await adminPut('/api/v0/admin/groups', {
				name: fname.trim(),
				description: fdescription,
				is_admin: fisAdmin,
				is_default: fisDefault,
				oidc_values: foidc.split(',').map((s) => s.trim()).filter(Boolean),
				tools: ftools.split(',').map((s) => s.trim()).filter(Boolean),
				skills: fskills.split(',').map((s) => s.trim()).filter(Boolean)
			});
			editing = null;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function remove(name: string) {
		if (!confirm(t('groups-delete-confirm', { name }))) return;
		try {
			await adminDelete(`/api/v0/admin/groups/${encodeURIComponent(name)}`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	onMount(refresh);
</script>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

{#if data}
	<div class="flex justify-between items-center mb-4">
		<p class="text-base-content/60 text-sm">{t('groups-intro')}</p>
		<button class="btn btn-primary btn-sm" onclick={openNew}>{t('groups-new-heading')}</button>
	</div>

	{#if editing !== null}
		<div class="card border border-base-300 mb-6">
			<div class="card-body">
				<h2 class="card-title text-base">
					{editing === '' ? t('groups-new-heading') : t('groups-edit-heading', { name: editing })}
				</h2>
				<div class="grid sm:grid-cols-2 gap-3">
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('groups-field-name')}</span>
						<input class="input input-bordered" bind:value={fname} disabled={editing !== ''} />
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('groups-field-description')}</span>
						<input class="input input-bordered" bind:value={fdescription} />
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('groups-field-oidc')}</span>
						<input class="input input-bordered" bind:value={foidc} placeholder="team-a, team-b" />
						{#if data.observed_oidc_values.length > 0}
							<span class="text-xs text-base-content/50">
								{t('groups-observed-values', { values: data.observed_oidc_values.join(', ') })}
							</span>
						{/if}
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('groups-field-tools')}</span>
						<input class="input input-bordered" bind:value={ftools} />
					</label>
					<label class="flex flex-col gap-1 sm:col-span-2">
						<span class="label-text">{t('groups-field-skills')}</span>
						<input class="input input-bordered" bind:value={fskills} />
					</label>
					<label class="label cursor-pointer gap-2">
						<input type="checkbox" class="toggle toggle-primary" bind:checked={fisAdmin} />
						<span class="label-text">{t('groups-field-admin')}</span>
					</label>
					<label class="label cursor-pointer gap-2">
						<input type="checkbox" class="toggle toggle-primary" bind:checked={fisDefault} />
						<span class="label-text">{t('groups-field-default')}</span>
					</label>
				</div>
				<div class="card-actions justify-end mt-2">
					<button class="btn btn-ghost btn-sm" onclick={() => (editing = null)}>{t('admin-cancel')}</button>
					<button class="btn btn-primary btn-sm" onclick={save} disabled={!fname.trim()}>
						{t('groups-save')}
					</button>
				</div>
			</div>
		</div>
	{/if}

	<ul class="flex flex-col gap-3">
		{#each data.groups as g (g.name)}
			<li>
				<div class="card border border-base-300">
					<div class="card-body py-3">
						<div class="flex items-center gap-3 flex-wrap">
							<span class="font-medium">{g.name}</span>
							{#if g.is_admin}<span class="badge badge-error badge-sm">admin</span>{/if}
							{#if g.is_default}<span class="badge badge-outline badge-sm">default</span>{/if}
							{#if g.description}<span class="text-base-content/60 text-sm">{g.description}</span>{/if}
							<span class="flex-1"></span>
							<span class="text-xs text-base-content/50">
								{t('groups-summary-counts', {
									oidc: g.oidc_values.length,
									tools: g.tools.length,
									skills: g.skills.length
								})}
							</span>
							<button class="btn btn-ghost btn-sm" onclick={() => openEdit(g)}>{t('rag-button-edit')}</button>
							<button class="btn btn-ghost btn-sm text-error" onclick={() => remove(g.name)}>
								{t('groups-delete')}
							</button>
						</div>
					</div>
				</div>
			</li>
		{/each}
	</ul>
{/if}
