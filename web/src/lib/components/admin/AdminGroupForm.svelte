<script lang="ts">
	import type { Priority } from '#lib/priority.js';
	import PrioritySelect from '#lib/components/PrioritySelect.svelte';
	import { untrack } from 'svelte';
	import { t } from '#lib/i18n.svelte.js';
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import { isWildcardSelected, multiSelectOptions, normalizeSelection } from '#lib/multi-select.js';

	export interface AdminGroup {
		name: string;
		priority: Priority | null;
		description: string;
		is_admin: boolean;
		is_default: boolean;
		can_manage_agents: boolean;
		can_manage_dots: boolean;
		oidc_values: string[];
		tools: string[];
		skills: string[];
	}

	export interface ToolFamily {
		id: string;
		/** The MCP connector's display name; empty for families that aren't per-server. */
		subject: string;
	}

	export interface McpTool {
		id: string;
		connector: string;
		description: string;
		category: string;
		order: number;
	}

	let { group = null, toolIds = [], toolFamilies = [], mcpTools = [], skillNames = [], observedOidcValues = [], seedOidcValue = '', onsave, ondelete }: {
		group?: AdminGroup | null;
		toolIds?: string[];
		toolFamilies?: ToolFamily[];
		mcpTools?: McpTool[];
		skillNames?: string[];
		observedOidcValues?: string[];
		/** Prefills the create form's claim values, from the identity tab. */
		seedOidcValue?: string;
		onsave: (group: AdminGroup) => Promise<void>;
		ondelete?: (name: string) => Promise<void>;
	} = $props();

	let name = $state(untrack(() => group?.name ?? ''));
	let description = $state(untrack(() => group?.description ?? ''));
	let priority = $state<Priority | null>(untrack(() => group?.priority ?? null));
	let isAdmin = $state(untrack(() => group?.is_admin ?? false));
	let isDefault = $state(untrack(() => group?.is_default ?? false));
	let canManageAgents = $state(untrack(() => group?.can_manage_agents ?? false));
	let canManageDots = $state(untrack(() => group?.can_manage_dots ?? false));
	let oidcValues = $state<string[]>(untrack(() => group?.oidc_values ?? (seedOidcValue ? [seedOidcValue] : [])));
	let newOidcValue = $state('');
	let tools = $state(untrack(() => normalizeSelection(group?.tools ?? [])));
	let skills = $state(untrack(() => normalizeSelection(group?.skills ?? [])));
	let oidcOptions = $derived(
		[...new Set([...observedOidcValues, ...oidcValues])].map((value) => ({ value, label: value }))
	);
	let saving = $state(false);
	let error = $state<string | null>(null);

	const asOptions = (ids: string[]) => ids.map((id) => ({ value: id, label: id }));
	// Families first: they are the late-binding grants, and a reader scanning for
	// "all of X" should meet them before a list of individual ids.
	let familyOptions = $derived(
		toolFamilies.map((family) => ({
			value: family.id,
			label:
				family.subject === ''
					? t('multi-select-family-comfyui')
					: t('multi-select-family-mcp', { subject: family.subject }),
			description: family.id,
			keywords: [family.subject]
		}))
	);
	// An MCP connector's own tools, so a group can be given part of a server
	// rather than all of it. Only what some connection has actually reported.
	let mcpToolOptions = $derived(
		mcpTools.map((tool) => ({
			value: tool.id,
			label: tool.id,
			description: tool.description,
			keywords: [tool.connector]
		}))
	);
	let grantableTools = $derived([...familyOptions, ...asOptions(toolIds), ...mcpToolOptions]);
	// Only worth saying when the operator has connectors but no tools to pick
	// from them — otherwise it is noise about a feature they are not using.
	let mcpToolsUnknown = $derived(
		mcpTools.length === 0 && toolFamilies.some((family) => family.subject !== '')
	);
	let toolOptions = $derived(
		multiSelectOptions(grantableTools, {
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
	let oidcSummary = $derived({
		empty: t('groups-oidc-none'),
		counted: (count: number) => t('groups-oidc-count', { count })
	});

	function addOidcValue() {
		const value = newOidcValue.trim();
		if (!value || oidcValues.includes(value)) return;
		oidcValues = [...oidcValues, value];
		newOidcValue = '';
	}
	// `*` on an admin group is what the setup wizard seeds and is meant there.
	// On any other group it silently widens with every release that adds a tool.
	let wildcardUnreviewed = $derived(!isAdmin && (isWildcardSelected(tools) || isWildcardSelected(skills)));

	async function save() {
		saving = true;
		error = null;
		try {
			await onsave({
				name: name.trim(),
				priority,
				description,
				is_admin: isAdmin,
				is_default: isDefault,
				can_manage_agents: canManageAgents,
				can_manage_dots: canManageDots,
				oidc_values: oidcValues,
				tools,
				skills
			});
			if (!group) {
				name = '';
				priority = null;
				description = '';
				isAdmin = false;
				isDefault = false;
				canManageAgents = false;
				canManageDots = false;
				oidcValues = [];
				newOidcValue = '';
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

	<div class="flex flex-col gap-3">
		<form class="flex flex-col gap-3" onsubmit={(event) => { event.preventDefault(); save(); }}>
			{#if error}<div class="alert alert-error py-2 text-sm"><span>{error}</span></div>{/if}
			<div class="grid grid-cols-1 gap-x-4 gap-y-3 sm:grid-cols-2">
				<label class="flex flex-col gap-1">
					<span class="label-text text-xs">{t('groups-field-name')}</span>
					<input class="input w-full font-mono {group ? 'bg-base-200' : ''}" bind:value={name} readonly={group !== null} required placeholder={group ? undefined : 'developers'} />
				</label>
				<label class="flex flex-col gap-1">
					<span class="label-text text-xs">{t('groups-field-description')}</span>
					<input class="input w-full" bind:value={description} />
				</label>
			</div>
			<PrioritySelect bind:value={priority} inheritLabel={t('priority-system-default')} />
			<p class="text-xs text-base-content/60">{t('priority-group-help')}</p>
			<div class="flex flex-wrap gap-4">
				<label class="label cursor-pointer justify-start gap-2"><input type="checkbox" class="checkbox checkbox-sm" bind:checked={isAdmin} /><span>{t('groups-field-admin')}</span></label>
				<label class="label cursor-pointer justify-start gap-2"><input type="checkbox" class="checkbox checkbox-sm" bind:checked={isDefault} /><span>{t('groups-field-default')}</span></label>
				<label class="label cursor-pointer justify-start gap-2" title={t('groups-field-manage-agents-help')}>
					<!-- Admin implies the permission, so an admin group shows it held rather than offering a switch that changes nothing. -->
					{#if isAdmin}
						<input type="checkbox" class="checkbox checkbox-sm" checked disabled />
					{:else}
						<input type="checkbox" class="checkbox checkbox-sm" bind:checked={canManageAgents} />
					{/if}
					<span>{t('groups-field-manage-agents')}</span>
				</label>
				<label class="label cursor-pointer justify-start gap-2" title={t('groups-field-manage-dots-help')}>
					{#if isAdmin}
						<input type="checkbox" class="checkbox checkbox-sm" checked disabled />
					{:else}
						<input type="checkbox" class="checkbox checkbox-sm" bind:checked={canManageDots} />
					{/if}
					<span>{t('groups-field-manage-dots')}</span>
				</label>
			</div>
			<span class="-mt-2 text-xs text-base-content/60">{t('groups-field-manage-agents-help')}</span>
			<span class="-mt-2 text-xs text-base-content/60">{t('groups-field-manage-dots-help')}</span>
			<div class="flex flex-col gap-2">
				<span class="label-text text-xs">{t('groups-field-oidc')}</span>
				<SearchableSelect multiple bind:values={oidcValues} options={oidcOptions} ariaLabel={t('groups-field-oidc')} summary={oidcSummary} placeholder={t('groups-oidc-none')} />
				{#if oidcValues.length}
					<ul class="flex flex-wrap gap-2" aria-label={t('groups-oidc-selected')}>
						{#each oidcValues as value (value)}
							<li class="badge badge-outline h-auto max-w-full gap-1 whitespace-normal py-1 text-left">
								<span class="break-all">{value}</span>
								<button class="btn btn-ghost btn-xs btn-circle" type="button" aria-label={t('groups-oidc-remove', { value })} onclick={() => (oidcValues = oidcValues.filter((item) => item !== value))}>×</button>
							</li>
						{/each}
					</ul>
				{/if}
				<div class="flex flex-col gap-2 sm:flex-row">
					<input class="input input-sm min-w-0 flex-1" bind:value={newOidcValue} aria-label={t('groups-oidc-add-label')} placeholder={t('groups-oidc-values-placeholder')} onkeydown={(event) => { if (event.key === 'Enter') { event.preventDefault(); addOidcValue(); } }} />
					<button class="btn btn-sm" type="button" disabled={!newOidcValue.trim() || oidcValues.includes(newOidcValue.trim())} onclick={addOidcValue}>{t('groups-oidc-add')}</button>
				</div>
				<span class="text-xs text-base-content/60">{t('groups-field-oidc-help')}</span>
			</div>
			<div class="flex flex-col gap-1">
				<span class="label-text text-xs">{t('groups-field-tools')}</span>
				<SearchableSelect multiple bind:values={tools} options={toolOptions} ariaLabel={t('groups-field-tools')} summary={summaryFor(t('multi-select-wildcard-tools'))} />
			</div>
			<div class="flex flex-col gap-1">
				<span class="label-text text-xs">{t('groups-field-skills')}</span>
				<SearchableSelect multiple bind:values={skills} options={skillOptions} ariaLabel={t('groups-field-skills')} summary={summaryFor(t('multi-select-wildcard-skills'))} />
			</div>
			{#if mcpToolsUnknown}
				<div class="alert alert-info py-2 text-xs"><span>{t('multi-select-mcp-none-cached')}</span></div>
			{/if}
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
