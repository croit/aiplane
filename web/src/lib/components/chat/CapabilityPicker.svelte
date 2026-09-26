<script lang="ts">
	import { base } from '$app/paths';
	import type { ChatCapability } from '$lib/api';
	import { capabilityCounts, filterCapabilities, type CapabilityStateFilter } from '$lib/capability-picker';
	import { t } from '$lib/i18n.svelte';
	import { toolCategoryLabel } from '$lib/tools';
	import SearchableSelect from '$lib/components/SearchableSelect.svelte';

	let { capabilities, onset, triggerLabel = null, dialogId = 'tool-selector-title', showActive = true }: {
		capabilities: ChatCapability[];
		onset: (capability: ChatCapability, state: ChatCapability['state']) => Promise<void>;
		triggerLabel?: string | null;
		dialogId?: string;
		showActive?: boolean;
	} = $props();

	let dialog: HTMLDialogElement;
	let open = $state(false);
	let query = $state('');
	let selectedGroup = $state('');
	let stateFilter = $state<CapabilityStateFilter>('all');
	let busy = $state(false);
	const active = $derived(capabilities.filter((capability) => capability.state === 'on'));
	const counts = $derived(capabilityCounts(capabilities));
	const groups = $derived(
		Array.from(new Set(capabilities.map((capability) => capability.group))).map((name) => ({
			name,
			rows: capabilities.filter((capability) => capability.group === name)
		}))
	);
	const shown = $derived(filterCapabilities(capabilities, {
		group: query.trim() ? null : selectedGroup || null,
		state: stateFilter,
		query
	}));
	const stateFilters: CapabilityStateFilter[] = ['all', 'on', 'auto', 'off'];
	let groupOptions = $derived([
		{ value: '', label: t('chat-render-all-tools-label'), description: t('chat-render-tool-count', { count: capabilities.length }) },
		...groups.map((group) => ({ value: group.name, label: toolCategoryLabel(group.name), description: t('chat-render-tool-count', { count: group.rows.length }) }))
	]);

	function stateLabel(state: CapabilityStateFilter): string {
		return t(`chat-render-state-${state}-label`);
	}

	function stateCount(state: CapabilityStateFilter): number {
		return state === 'all' ? capabilities.length : counts[state];
	}

	function openPicker() {
		open = true;
		dialog.showModal();
	}

	function closePicker() {
		dialog.close();
	}

	async function setMany(rows: ChatCapability[], state: ChatCapability['state']) {
		busy = true;
		try {
			for (const row of rows) {
				await onset(row, state === 'off' && !row.can_disable ? 'auto' : state);
			}
		} finally {
			busy = false;
		}
	}

	function aggregate(rows: ChatCapability[]): ChatCapability['state'] | null {
		const first = rows[0]?.state;
		return first && rows.every((row) => row.state === first) ? first : null;
	}
</script>

{#snippet segmented(rows: ChatCapability[])}
	{@const selected = aggregate(rows)}
	<div class="join w-full shrink-0 sm:w-auto" aria-label={t('chat-render-tools-state-label')}>
		{#if rows.some((row) => row.can_disable)}
			<button type="button" class="btn btn-sm join-item flex-1 sm:flex-none {selected === 'off' ? 'btn-active' : 'btn-ghost'}" title={t('chat-render-state-off-tip')} aria-label={t('chat-render-state-off-tip')} aria-pressed={selected === 'off'} disabled={busy} onclick={() => setMany(rows, 'off')}>{stateLabel('off')}</button>
		{/if}
		<button type="button" class="btn btn-sm join-item flex-1 sm:flex-none {selected === 'auto' ? 'btn-active' : 'btn-ghost'}" title={t('chat-render-state-auto-tip')} aria-label={t('chat-render-state-auto-tip')} aria-pressed={selected === 'auto'} disabled={busy} onclick={() => setMany(rows, 'auto')}>{stateLabel('auto')}</button>
		<button type="button" class="btn btn-sm join-item flex-1 sm:flex-none {selected === 'on' ? 'btn-active' : 'btn-ghost'}" title={t('chat-render-state-on-tip')} aria-label={t('chat-render-state-on-tip')} aria-pressed={selected === 'on'} disabled={busy} onclick={() => setMany(rows, 'on')}>{stateLabel('on')}</button>
	</div>
{/snippet}

<div class="relative flex flex-wrap items-center gap-1.5">
	<button type="button" class="btn btn-ghost btn-sm gap-1 rounded-full" title={t('chat-render-tools-tooltip')} onclick={openPicker} aria-expanded={open}>
		<span aria-hidden="true">+</span> {triggerLabel ?? t('chat-render-tools-label')}
	</button>
	{#if showActive && active.length > 0}
		<button type="button" class="badge badge-outline gap-1 sm:hidden" title={t('chat-render-active-count-title')} onclick={openPicker}>⌁ {active.length}</button>
	{/if}
	{#each showActive ? active : [] as capability (`${capability.kind}:${capability.key}`)}
		<button type="button" class="badge badge-outline hidden gap-1 sm:inline-flex" title={t('chat-render-unpin-title')} onclick={() => onset(capability, 'auto')}>
			{capability.title} <span class="opacity-60">×</span>
		</button>
	{/each}

	<dialog bind:this={dialog} class="modal p-0" aria-labelledby={dialogId} onclose={() => (open = false)} oncancel={(event) => { event.preventDefault(); closePicker(); }}>
		<div class="modal-box flex h-dvh max-h-dvh w-screen max-w-none flex-col rounded-none border-0 bg-base-100 p-0">
			<header class="flex min-h-16 items-center gap-3 border-b border-base-300 px-4 sm:px-6">
				<div class="min-w-0 flex-1">
					<h2 class="text-xl font-semibold" id={dialogId}>{t('chat-render-tools-label')}</h2>
					<p class="text-sm text-base-content/60">{t('chat-render-tools-summary', counts)}</p>
				</div>
				<button type="button" class="btn btn-ghost btn-circle" aria-label={t('chat-render-close')} onclick={closePicker}>×</button>
			</header>

			{#if capabilities.length > 0}
				<div class="flex flex-col gap-3 border-b border-base-300 px-4 py-3 sm:px-6 lg:flex-row lg:items-center">
					<label class="input w-full lg:max-w-xl">
						<span aria-hidden="true">⌕</span>
						<input bind:value={query} placeholder={t('chat-render-tools-search-placeholder')} />
					</label>
					<div role="tablist" class="tabs tabs-box tabs-sm max-w-full overflow-x-auto">
						{#each stateFilters as state (state)}
							<button type="button" role="tab" class="tab gap-1 whitespace-nowrap {stateFilter === state ? 'tab-active' : ''}" aria-selected={stateFilter === state} onclick={() => (stateFilter = state)}>
								{stateLabel(state)} <span class="badge badge-sm badge-ghost">{stateCount(state)}</span>
							</button>
						{/each}
					</div>
				</div>

				<div class="grid min-h-0 flex-1 md:grid-cols-[16rem_minmax(0,1fr)]">
					<nav class="hidden overflow-y-auto border-r border-base-300 bg-base-200/25 p-3 md:block" aria-label={t('chat-render-tools-category-label')}>
						<ul class="menu w-full gap-1">
							<li><button type="button" class={selectedGroup === '' ? 'menu-active' : ''} onclick={() => { selectedGroup = ''; query = ''; }}><span class="min-w-0 flex-1 truncate">{t('chat-render-all-tools-label')}</span><span class="badge badge-sm">{capabilities.length}</span></button></li>
							{#each groups as group (group.name)}
								<li><button type="button" class={selectedGroup === group.name ? 'menu-active' : ''} onclick={() => { selectedGroup = group.name; query = ''; }}><span class="min-w-0 flex-1 truncate">{toolCategoryLabel(group.name)}</span><span class="badge badge-sm">{group.rows.length}</span></button></li>
							{/each}
						</ul>
					</nav>

					<section class="flex min-h-0 min-w-0 flex-col" aria-labelledby={dialogId}>
						<div class="border-b border-base-300 p-3 md:hidden">
							<SearchableSelect options={groupOptions} bind:value={selectedGroup} onchange={() => (query = '')} ariaLabel={t('chat-render-tools-category-label')} class="w-full" />
						</div>
						<div class="flex flex-col gap-3 border-b border-base-300 px-4 py-3 sm:flex-row sm:items-center sm:px-6">
							<div class="min-w-0 flex-1">
								<h3 class="truncate text-lg font-semibold">{query.trim() ? t('chat-render-tools-search-results') : selectedGroup ? toolCategoryLabel(selectedGroup) : t('chat-render-all-tools-label')}</h3>
								<p class="text-sm text-base-content/60">{t('chat-render-tool-count', { count: shown.length })}</p>
							</div>
							{#if !query.trim() && shown.length > 0}
								<div class="flex flex-col gap-1 sm:items-end">
									<span class="text-xs font-medium text-base-content/60">{t('chat-render-tools-set-group')}</span>
									{@render segmented(shown)}
								</div>
							{/if}
						</div>

						<div class="min-h-0 flex-1 overflow-y-auto px-4 sm:px-6">
							{#if shown.length > 0}
								<ul class="divide-y divide-base-300/60">
									{#each shown as capability (`${capability.kind}:${capability.key}`)}
										<li class="flex flex-col gap-3 py-4 sm:flex-row sm:items-center">
											<div class="min-w-0 flex-1">
												<h4 class="font-medium">{capability.title}</h4>
												{#if capability.description}<p class="mt-1 max-w-3xl text-sm text-base-content/60">{capability.description}</p>{/if}
											</div>
											{@render segmented([capability])}
										</li>
									{/each}
								</ul>
							{:else}
								<div class="flex h-full min-h-48 items-center justify-center text-center text-base-content/60">{t('chat-render-tools-empty')}</div>
							{/if}
						</div>
					</section>
				</div>

				<footer class="flex min-h-16 items-center gap-3 border-t border-base-300 px-4 sm:px-6">
					<p class="min-w-0 flex-1 truncate text-sm text-base-content/60">{t('chat-render-tools-summary', counts)}</p>
					<button type="button" class="btn" onclick={closePicker}>{t('chat-render-tools-done')}</button>
				</footer>
			{:else}
				<div class="flex flex-1 items-center justify-center p-6 text-sm">
					<p>{t('chat-render-no-tools-prefix')} <a class="link" href="{base}/integrations">{t('nav-integrations')}</a>{t('chat-render-no-tools-suffix')}</p>
				</div>
			{/if}
		</div>
	</dialog>
</div>
