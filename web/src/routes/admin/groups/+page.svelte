<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { base } from '$app/paths';
	import { goto } from '$app/navigation';
	import { adminDelete, adminJson, adminPut } from '$lib/admin-client';
	import { selectedGroupAdminTab, skillMatrixRows, toolMatrixRows } from '$lib/admin-groups';
	import AdminGroupForm from '$lib/components/admin/AdminGroupForm.svelte';
	import AdminGrantMatrix from '$lib/components/admin/AdminGrantMatrix.svelte';
	import AdminIdentityMapping from '$lib/components/admin/AdminIdentityMapping.svelte';
	import type { AdminGroup, McpTool, ToolFamily } from '$lib/components/admin/AdminGroupForm.svelte';
	import { t } from '$lib/i18n.svelte';

	interface GroupsData {
		groups: AdminGroup[];
		observed_oidc_values: string[];
		tool_ids: string[];
		tool_families: ToolFamily[];
		mcp_tools: McpTool[];
		skill_names: string[];
	}

	let data = $state<GroupsData | null>(null);
	let error = $state<string | null>(null);
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

	async function remove(name: string) {
		if (!confirm(t('groups-delete-confirm', { name }))) return;
		await adminDelete(`/api/v0/admin/groups/${encodeURIComponent(name)}`);
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
		await goto(`${base}/admin/groups?tab=groups`);
	}

	let toolRows = $derived(
		toolMatrixRows(data?.tool_ids ?? [], data?.tool_families ?? [], data?.mcp_tools ?? [], {
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
			<h1 class="text-2xl font-bold">{t('groups-heading')}</h1>
			<p class="max-w-3xl text-sm text-base-content/70">{t('groups-intro')}</p>
		</div>
		<nav class="tabs tabs-border w-full overflow-x-auto" aria-label={t('groups-heading')}>
			<a class:tab-active={selected === 'groups'} class="tab whitespace-nowrap" href="{base}/admin/groups?tab=groups">{t('groups-tab-groups')}</a>
			<a class:tab-active={selected === 'identity'} class="tab whitespace-nowrap" href="{base}/admin/groups?tab=identity">{t('groups-tab-identity')}</a>
			<a class:tab-active={selected === 'tools'} class="tab whitespace-nowrap" href="{base}/admin/groups?tab=tools">{t('groups-tab-tools')}</a>
			<a class:tab-active={selected === 'skills'} class="tab whitespace-nowrap" href="{base}/admin/groups?tab=skills">{t('groups-tab-skills')}</a>
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
			{#key seedOidcValue}
				<AdminGroupForm seedOidcValue={seedOidcValue} toolIds={data.tool_ids} toolFamilies={data.tool_families} mcpTools={data.mcp_tools} skillNames={data.skill_names} onsave={save} />
			{/key}
			<div class="flex flex-col gap-4">
				<h2 class="text-lg font-semibold">{t('groups-existing-heading')}</h2>
				{#if data.groups.length === 0}
					<p class="text-sm text-base-content/60">{t('groups-empty')}</p>
				{:else}
					{#each data.groups as group (group.name)}
						<AdminGroupForm {group} toolIds={data.tool_ids} toolFamilies={data.tool_families} mcpTools={data.mcp_tools} skillNames={data.skill_names} onsave={save} ondelete={remove} />
					{/each}
				{/if}
			</div>
		{/if}
	{/if}
</section>
