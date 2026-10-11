<script lang="ts">
	import { dotAccessApi, type Dot, type DotAccessTool, type DotToolPermission } from '#lib/dots.js';
	import { searchCapabilities } from '#lib/capability-picker.js';
	import { t } from '#lib/i18n.svelte.js';

	let { dot }: { dot: Dot } = $props();
	let tools = $state<DotAccessTool[]>([]);
	let loaded = $state(false);
	let loading = $state(false);
	let saving = $state(false);
	let dirty = $state(false);
	let query = $state('');
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	const shown = $derived(searchCapabilities(tools.map((tool) => ({
		...tool, key: tool.id, kind: 'tool', title: tool.label, group: tool.connector_key ?? '', order: 0, icon: null
	})), query));

	async function refresh() {
		loading = true;
		error = null;
		notice = null;
		try {
			tools = (await dotAccessApi.load(dot.id)).tools;
			loaded = true;
			dirty = false;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : t('dot-access-error-load');
		} finally {
			loading = false;
		}
	}

	function onToggle(event: Event) {
		const details = event.currentTarget;
		if (details instanceof HTMLDetailsElement && details.open && !loaded && !loading) void refresh();
	}

	function change(id: string, patch: { selected?: boolean; permission?: DotToolPermission }) {
		tools = tools.map((tool) => tool.id === id ? { ...tool, ...patch } : tool);
		dirty = true;
		notice = null;
	}

	async function save() {
		saving = true;
		error = null;
		notice = null;
		try {
			tools = (await dotAccessApi.save(dot.id, tools.filter((tool) => tool.selected).map(({ id, permission }) => ({ id, permission })))).tools;
			dirty = false;
			notice = t('dot-access-saved');
		} catch (caught) {
			error = caught instanceof Error ? caught.message : t('dot-access-error-save');
		} finally {
			saving = false;
		}
	}
</script>

<details class="collapse collapse-arrow border border-base-300 bg-base-100" ontoggle={onToggle}>
	<summary class="collapse-title min-h-0 py-3 text-sm font-medium">{t('dot-access-heading')}</summary>
	<div class="collapse-content min-w-0 space-y-3 text-sm">
		<p class="text-base-content/60">{t('dot-access-scope', { name: dot.display || dot.name })}</p>
		{#if notice}<div class="alert alert-success py-2" role="status"><span>{notice}</span></div>{/if}
		{#if error}<div class="alert alert-error py-2" role="alert"><span class="break-words">{error}</span></div>{/if}
		{#if loading}
			<div class="skeleton h-16 w-full"></div>
		{:else if loaded}
			<label class="input input-sm w-full"><input bind:value={query} aria-label={t('chat-render-tools-search-placeholder')} placeholder={t('chat-render-tools-search-placeholder')} /></label>
			<ul class="max-h-96 space-y-3 overflow-y-auto">
				{#each shown as tool (tool.id)}
					<li class="rounded-box border border-base-300 p-3">
						<label class="flex items-start gap-2">
							<input class="checkbox checkbox-sm mt-0.5 shrink-0" type="checkbox" checked={tool.selected} disabled={saving} aria-label={t('tools-toggle-aria', { name: `${tool.label} (${tool.id})` })} onchange={(event) => change(tool.id, { selected: event.currentTarget.checked })} />
							<span class="min-w-0 break-words font-medium">{tool.label}<code class="mt-1 block font-mono text-xs font-normal text-base-content/50">{tool.id}</code></span>
						</label>
						{#if tool.description}
							<details class="collapse collapse-arrow mt-2 bg-base-200">
								<summary class="collapse-title min-h-0 py-2 text-xs">{t('dots-create-description')}</summary>
								<div class="collapse-content"><p class="break-words text-xs text-base-content/60">{tool.description}</p></div>
							</details>
						{/if}
						<p class="mt-2 break-words text-xs text-base-content/60">{t(`dot-access-identity-${tool.identity}`)}{#if tool.connector_key} · {tool.connector_key}{/if}</p>
						<label class="mt-2 flex flex-col gap-1">
							<span class="text-xs">{t('dot-access-rule')}</span>
							<select class="select select-sm w-full" value={tool.permission} disabled={!tool.selected || saving} onchange={(event) => change(tool.id, { permission: event.currentTarget.value as DotToolPermission })}>
								<option value="always_allow">{t('dot-access-allow')}</option>
								<option value="always_ask">{t('dot-access-ask')}</option>
							</select>
						</label>
						{#if tool.connector_mode === 'ask'}<p class="mt-1 text-xs text-base-content/60">{t('dot-access-connector-asks')}</p>{/if}
					</li>
				{:else}
					<li class="text-base-content/60">{tools.length ? t('searchable-select-no-results') : t('dot-access-empty')}</li>
				{/each}
			</ul>
			<p class="text-xs text-base-content/60">{t('dot-access-pending')}</p>
		{/if}
		<div class="flex flex-wrap justify-end gap-2">
			<button class="btn btn-ghost btn-sm" type="button" disabled={loading || saving} onclick={() => void refresh()}>{t('dot-access-reload')}</button>
			{#if loaded}<button class="btn btn-sm" type="button" disabled={!dirty || loading || saving} onclick={() => void save()}>{saving ? t('dot-access-saving') : t('memory-save-button')}</button>{/if}
		</div>
	</div>
</details>
