<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import Modal from '#lib/components/ui/Modal.svelte';
	import { adminDelete, adminJson, adminPut } from '#lib/admin-client.js';
	import { selectedGroupAdminTab, skillMatrixRows, toolMatrixRows } from '#lib/admin-groups.js';
	import AdminGroupForm from '#lib/components/admin/AdminGroupForm.svelte';
	import AdminGrantMatrix from '#lib/components/admin/AdminGrantMatrix.svelte';
	import AdminIdentityMapping from '#lib/components/admin/AdminIdentityMapping.svelte';
	import type { GrantableTool } from '#lib/admin-groups.js';
	import type { AdminGroup, McpTool, ToolFamily } from '#lib/components/admin/AdminGroupForm.svelte';
	import { t } from '#lib/i18n.svelte.js';

	interface GroupsData {
		groups: AdminGroup[];
		observed_oidc_values: string[];
		tools: GrantableTool[];
		tool_families: ToolFamily[];
		mcp_tools: McpTool[];
		skill_names: string[];
	}

	let data = $state<GroupsData | null>(null);
	let error = $state<string | null>(null);
	let creating = $state(false);
	let editing = $state<AdminGroup | null>(null);
	let editOpen = $state(false);
	/** Prefills the create form when the identity tab sends a value over. */
	let seedOidcValue = $state('');

	let selected = $derived(selectedGroupAdminTab(page.url.search));

	async function refresh() {
		try {
			data = await adminJson<GroupsData>('/api/v0/admin/groups');
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	async function save(group: AdminGroup) {
		await adminPut('/api/v0/admin/groups', group);
		await refresh();
	}

	async function createGroup(group: AdminGroup) {
		await save(group);
		creating = false;
		seedOidcValue = '';
	}

	async function editGroup(group: AdminGroup) {
		await save(group);
		editing = null;
		editOpen = false;
	}

	async function remove(name: string) {
		if (!confirm(t('groups-delete-confirm', { name }))) return;
		await adminDelete(`/api/v0/admin/groups/${encodeURIComponent(name)}`);
		editing = null;
		editOpen = false;
		await refresh();
	}

	// The matrix edits one list of one group; everything else about the group has
	// to be resent unchanged, because the save replaces the whole row.
	async function saveTools(group: AdminGroup, tools: string[]) {
		await save({ ...group, tools });
	}

	async function saveSkills(group: AdminGroup, skills: string[]) {
		await save({ ...group, skills });
	}

	async function createFrom(value: string) {
		seedOidcValue = value;
		await goto('/admin/groups?tab=groups');
		creating = true;
	}

	let toolRows = $derived(
		toolMatrixRows(data?.tools ?? [], data?.tool_families ?? [], data?.mcp_tools ?? [], {
			wildcard: t('groups-matrix-wildcard-tools'),
			family: (family) =>
				family.subject === ''
					? t('multi-select-family-comfyui')
					: t('multi-select-family-mcp', { subject: family.subject })
		})
	);
	let skillRows = $derived(
		skillMatrixRows(data?.skill_names ?? [], { wildcard: t('groups-matrix-wildcard-skills') })
	);

	onMount(refresh);
</script>

<section class="flex w-full flex-col gap-4">
	<header class="flex flex-col gap-3">
		<div>
			<h2 class="text-xl font-semibold">{t('groups-heading')}</h2>
			<p class="max-w-3xl text-sm text-base-content/70">{t('groups-intro')}</p>
		</div>
		<nav class="tabs tabs-border w-full overflow-x-auto" aria-label={t('groups-heading')}>
			<a class:tab-active={selected === 'groups'} class="tab whitespace-nowrap" href="/admin/groups?tab=groups">{t('groups-tab-groups')}</a>
			<a class:tab-active={selected === 'identity'} class="tab whitespace-nowrap" href="/admin/groups?tab=identity">{t('groups-tab-identity')}</a>
			<a class:tab-active={selected === 'tools'} class="tab whitespace-nowrap" href="/admin/groups?tab=tools">{t('groups-tab-tools')}</a>
			<a class:tab-active={selected === 'skills'} class="tab whitespace-nowrap" href="/admin/groups?tab=skills">{t('groups-tab-skills')}</a>
		</nav>
	</header>

	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if data}
		<datalist id="group-oidc-values">{#each data.observed_oidc_values as value}<option value={value}></option>{/each}</datalist>

		{#if selected === 'identity'}
			<AdminIdentityMapping observed={data.observed_oidc_values} groups={data.groups} oncreate={createFrom} />
		{:else if selected === 'tools'}
			<AdminGrantMatrix rows={toolRows} groups={data.groups} held={(group) => group.tools} onsave={saveTools} />
		{:else if selected === 'skills'}
			<AdminGrantMatrix rows={skillRows} groups={data.groups} held={(group) => group.skills} onsave={saveSkills} />
		{:else}
			<header class="flex flex-wrap items-center gap-3">
				<h3 class="text-lg font-semibold">{t('groups-existing-heading')}</h3>
				<button class="btn btn-primary btn-sm ml-auto" type="button" onclick={() => (creating = true)}>{t('groups-add')}</button>
			</header>
			{#if data.groups.length === 0}
				<p class="text-sm text-base-content/60">{t('groups-empty')}</p>
			{:else}
				<div class="grid min-w-0 gap-3 md:grid-cols-2">
					{#each data.groups as group (group.name)}
						<article class="card card-border bg-base-100">
							<div class="card-body gap-3 p-4">
								<div class="min-w-0">
									<h4 class="card-title break-words font-mono text-base">{group.name}</h4>
									{#if group.description}<p class="mt-2 whitespace-pre-wrap break-words text-sm text-base-content/70">{group.description}</p>{/if}
								</div>
								<div class="flex flex-wrap gap-2">
									{#if group.is_admin}<span class="badge badge-outline">{t('groups-field-admin')}</span>{/if}
									{#if group.is_default}<span class="badge badge-outline">{t('groups-field-default')}</span>{/if}
									{#if group.can_manage_agents || group.is_admin}<span class="badge badge-outline">{t('groups-field-manage-agents')}</span>{/if}
									{#if group.can_manage_dots || group.is_admin}<span class="badge badge-outline">{t('groups-field-manage-dots')}</span>{/if}
									{#if group.tools.length}<span class="badge badge-outline">{t('groups-field-tools')}: {group.tools.length}</span>{/if}
									{#if group.skills.length}<span class="badge badge-outline">{t('groups-field-skills')}: {group.skills.length}</span>{/if}
								</div>
								<div class="card-actions justify-end">
									<button class="btn btn-sm" type="button" onclick={() => { editing = group; editOpen = true; }}>{t('groups-edit')}</button>
								</div>
							</div>
						</article>
					{/each}
				</div>
			{/if}
		{/if}
	{/if}
</section>

<Modal bind:open={creating} title={t('groups-new-heading')} size="xl">
	{#if data}
		{#key seedOidcValue}
			<AdminGroupForm seedOidcValue={seedOidcValue} toolIds={data.tools.map((tool) => tool.id)} toolFamilies={data.tool_families} mcpTools={data.mcp_tools} skillNames={data.skill_names} onsave={createGroup} />
		{/key}
	{/if}
</Modal>

<Modal bind:open={editOpen} title={editing?.name ?? t('groups-existing-heading')} size="xl">
	{#if data && editing}
		{#key editing.name}
			<AdminGroupForm group={editing} toolIds={data.tools.map((tool) => tool.id)} toolFamilies={data.tool_families} mcpTools={data.mcp_tools} skillNames={data.skill_names} onsave={editGroup} ondelete={remove} />
		{/key}
	{/if}
</Modal>
