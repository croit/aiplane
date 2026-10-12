<script lang="ts">
	import { dotAccessApi, type Dot, type DotAccessTool, type DotToolPermission, type DotActionRule } from '#lib/dots.js';
	import { actionRows, setActionRule, accessSelections } from '#lib/dot-action-rules.js';
	import { searchCapabilities } from '#lib/capability-picker.js';
	import { ApiError } from '#lib/api.js';
	import DotConnectionsPanel from './DotConnectionsPanel.svelte';
	import { t } from '#lib/i18n.svelte.js';

	let { dot, initiallyOpen = false, tabbed = false, active = false }: { dot: Dot; initiallyOpen?: boolean; tabbed?: boolean; active?: boolean } = $props();
	let stale = $state(false);
	// svelte-ignore state_referenced_locally
	let open = $state(initiallyOpen);
	let tools = $state<DotAccessTool[]>([]);
	let loaded = $state(false);
	let loading = $state(false);
	let saving = $state(false);
	let dirty = $state(false);
	let query = $state('');
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let openActions = $state<Record<string, boolean>>({});
	const selections = $derived(accessSelections(tools));
	const shown = $derived(searchCapabilities(tools.map((tool) => ({
		...tool, key: tool.id, kind: 'tool', title: tool.label ?? tool.id, description: tool.description ?? '', group: tool.connector_key ?? '', order: 0, icon: null
	})), query));

	async function refresh() {
		loading = true;
		error = null;
		notice = null;
		try {
			tools = (await dotAccessApi.load(dot.id)).tools;
			loaded = true;
			dirty = false;
			stale = false;
		} catch (caught) {
			error = caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : t('dot-access-error-load');
		} finally {
			loading = false;
		}
	}

	function onToggle(event: Event) {
		const details = event.currentTarget;
		if (details instanceof HTMLDetailsElement && details.open && !loaded && !loading) void refresh();
	}

	$effect(() => {
		if (tabbed && active && !loaded && !loading && !error) void refresh();
	});

	function change(id: string, patch: { selected?: boolean; permission?: DotToolPermission }) {
		tools = tools.map((tool) => tool.id === id ? { ...tool, ...patch } : tool);
		dirty = true;
		notice = null;
	}

	function changeAction(toolId: string, actionId: string, rule: DotActionRule | null) {
		tools = tools.map((tool) => tool.id === toolId ? setActionRule(tool, actionId, rule) : tool);
		dirty = true;
		notice = null;
	}

	function toggleActions(toolId: string, event: Event) {
		const details = event.currentTarget;
		if (details instanceof HTMLDetailsElement && details.isConnected) {
			openActions[toolId] = details.open;
		}
	}

	async function save() {
		if (selections === null || stale) return;
		saving = true;
		error = null;
		notice = null;
		try {
			tools = (await dotAccessApi.save(dot.id, selections)).tools;
			dirty = false;
			notice = t('dot-access-saved');
		} catch (caught) {
			error = caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : t('dot-access-error-save');
		} finally {
			saving = false;
		}
	}
</script>

{#snippet panelBody()}
	<div class="min-w-0 space-y-3 text-sm">
		<p class="text-base-content/60">{t('dot-access-scope', { name: dot.display || dot.name })}</p>
		{#if notice}<div class="alert alert-success py-2" role="status"><span>{notice}</span></div>{/if}
		{#if error}<div class="alert alert-error py-2" role="alert"><span class="break-words">{error}</span></div>{/if}
		{#if loaded}<DotConnectionsPanel {dot} onchange={() => stale = true} />{/if}
		{#if stale}<div class="alert alert-warning py-2" role="alert"><span>{t('dot-connection-access-reload')}</span></div>{/if}
		{#if loading}
			<div class="skeleton h-16 w-full"></div>
		{:else if loaded}
			{#if selections === null}<div class="alert alert-warning py-2" role="alert"><span>{t('dot-action-correction-required')}</span></div>{/if}
			<label class="input input-sm w-full"><input bind:value={query} aria-label={t('chat-render-tools-search-placeholder')} placeholder={t('chat-render-tools-search-placeholder')} /></label>
			<ul class="max-h-96 space-y-3 overflow-y-auto">
				{#each shown as tool (tool.id)}
					<li class="rounded-box border border-base-300 p-3">
						<label class="flex items-start gap-2">
							<input class="checkbox checkbox-sm mt-0.5 shrink-0" type="checkbox" checked={tool.selected} disabled={saving || (!tool.selected && !tool.available)} aria-label={t('tools-toggle-aria', { name: tool.label ? `${tool.label} (${tool.id})` : tool.id })} onchange={(event) => change(tool.id, { selected: event.currentTarget.checked })} />
							<span class="min-w-0 break-words font-medium">{tool.label ?? tool.id}<code class="mt-1 block font-mono text-xs font-normal text-base-content/50">{tool.id}</code></span>
						</label>
						{#if tool.description}
							<details class="collapse collapse-arrow mt-2 bg-base-200">
								<summary class="collapse-title min-h-0 py-2 text-xs">{t('dots-create-description')}</summary>
								<div class="collapse-content"><p class="break-words text-xs text-base-content/60">{tool.description}</p></div>
							</details>
						{/if}
						<p class="mt-2 break-words text-xs text-base-content/60">{tool.identity === 'unavailable' ? t('dot-access-unavailable') : t(`dot-access-identity-${tool.identity}`, { name: dot.display || dot.name })}{#if tool.connector_key} · {tool.connector_key}{/if}</p>
						<label class="mt-2 flex flex-col gap-1">
							<span class="text-xs">{t('dot-access-rule')}</span>
							<select class="select select-sm w-full" value={tool.permission} disabled={!tool.selected || !tool.available || saving} onchange={(event) => change(tool.id, { permission: event.currentTarget.value as DotToolPermission })}>
								<option value="always_allow">{t('dot-access-allow')}</option>
								<option value="always_ask">{t('dot-access-ask')}</option>
							</select>
						</label>
						{#if tool.connector_mode === 'ask'}<p class="mt-1 text-xs text-base-content/60">{t('dot-access-connector-asks')}</p>{/if}
						{#if tool.actions?.length || tool.action_rules.length}
							<details class="collapse collapse-arrow mt-2 border border-base-300" open={openActions[tool.id] ?? false} ontoggle={(event) => toggleActions(tool.id, event)}>
								<summary class="collapse-title min-h-0 py-2 text-xs font-medium"><span>{t('dot-action-heading')}</span><span class="badge badge-outline badge-sm ml-2">{t('dot-action-count', { count: tool.action_rules.length })}</span></summary>
								<div class="collapse-content min-w-0 space-y-3">
									<p class="text-xs text-base-content/60">{t('dot-action-inheritance')}</p>
									<ul class="space-y-3">
										{#each actionRows(tool) as action (action.id)}
											<li class="rounded-box border border-base-300 p-2">
												<code class="block break-all text-xs">{action.id}</code>
												{#if action.description}<p class="mt-1 break-words text-xs text-base-content/60">{action.description}</p>{/if}
												{#if !action.available}
													<p class="mt-1 text-xs text-warning">{t('dot-action-unavailable')}</p>
													<code class="mt-1 block break-all text-xs">{action.rule}</code>
													<button class="btn btn-ghost btn-xs mt-1" type="button" disabled={saving} aria-label={t('dot-action-remove-aria', { action: action.id })} onclick={() => changeAction(tool.id, action.id, null)}>{t('dot-action-remove')}</button>
												{:else}
													<label class="mt-2 flex flex-col gap-1">
														<span class="text-xs">{t('dot-action-rule', { action: action.id })}</span>
														<select class="select select-sm w-full" value={action.rule ?? ''} disabled={!tool.selected || !tool.available || saving} onchange={(event) => changeAction(tool.id, action.id, (event.currentTarget.value || null) as DotActionRule | null)}>
															{#if action.rule && !['allow', 'ask', 'deny'].includes(action.rule)}<option value={action.rule} disabled>{t('dot-action-unknown-rule', { rule: action.rule })}</option>{/if}
															<option value="">{t('dot-action-inherit')}</option>
															<option value="allow">{t('dot-access-allow')}</option>
															<option value="ask">{t('dot-access-ask')}</option>
															<option value="deny">{t('dot-action-deny')}</option>
														</select>
													</label>
												{/if}
											</li>
										{/each}
									</ul>
								</div>
							</details>
						{/if}
					</li>
				{:else}
					<li class="text-base-content/60">{tools.length ? t('searchable-select-no-results') : t('dot-access-empty')}</li>
				{/each}
			</ul>
			<p class="text-xs text-base-content/60">{t('dot-access-pending')}</p>
		{/if}
		<div class="flex flex-wrap justify-end gap-2">
			<button class="btn btn-ghost btn-sm" type="button" disabled={loading || saving} onclick={() => void refresh()}>{t('dot-access-reload')}</button>
			{#if loaded}<button class="btn btn-sm" type="button" disabled={!dirty || loading || saving || selections === null || stale} onclick={() => void save()}>{saving ? t('dot-access-saving') : t('memory-save-button')}</button>{/if}
		</div>
	</div>
{/snippet}

{#if tabbed}
	<section hidden={!active} class="min-w-0" data-dot-panel="access">{@render panelBody()}</section>
{:else}
	<details class="collapse collapse-arrow border border-base-300 bg-base-100" bind:open ontoggle={onToggle}>
		<summary class="collapse-title min-h-0 py-3 text-sm font-medium">{t('dot-access-heading')}</summary>
		<div class="collapse-content">{@render panelBody()}</div>
	</details>
{/if}
