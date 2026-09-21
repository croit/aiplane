<script lang="ts">
	import { onMount } from 'svelte';
	import { adminDelete, adminJson, adminPut } from '$lib/admin-client';
	import AdminGroupForm from '$lib/components/admin/AdminGroupForm.svelte';
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

	onMount(refresh);
</script>

<section class="flex w-full flex-col gap-4">
	<header class="flex flex-col gap-1">
		<h1 class="text-2xl font-bold">{t('groups-heading')}</h1>
		<p class="text-sm text-base-content/70">{t('groups-intro')}</p>
	</header>

	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if data}
		<datalist id="group-oidc-values">{#each data.observed_oidc_values as value}<option value={value}></option>{/each}</datalist>

		<AdminGroupForm toolIds={data.tool_ids} toolFamilies={data.tool_families} mcpTools={data.mcp_tools} skillNames={data.skill_names} onsave={save} />
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
</section>
