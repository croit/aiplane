<script lang="ts">
	import type { ChatCapability } from '#lib/api.js';
	import { capabilityCounts, fittingCapabilities, type CapabilityStateFilter } from '#lib/capability-picker.js';
	import { t } from '#lib/i18n.svelte.js';
	import CapabilityBrowser from '#lib/components/capabilities/CapabilityBrowser.svelte';

	/** `showTrigger: false` is for a caller that opens the picker itself
	 *  through `show()` — from inside another dialog, whose box would
	 *  otherwise contain and restyle this full-screen one. */
	let { capabilities, onset, triggerLabel = null, dialogId = 'tool-selector-title', showActive = true, showTrigger = true }: {
		capabilities: ChatCapability[];
		onset: (capability: ChatCapability, state: ChatCapability['state']) => Promise<void>;
		triggerLabel?: string | null;
		dialogId?: string;
		showActive?: boolean;
		showTrigger?: boolean;
	} = $props();

	export function show() {
		openPicker();
	}

	let dialog: HTMLDialogElement;
	let open = $state(false);
	let stateFilter = $state<CapabilityStateFilter>('all');
	let busy = $state(false);
	const active = $derived(capabilities.filter((capability) => capability.state === 'on'));
	const counts = $derived(capabilityCounts(capabilities));
	const stateFilters: CapabilityStateFilter[] = ['all', 'on', 'auto', 'off'];

	// The pinned badges must never wrap the composer taller: the row keeps the
	// first N that fit and folds the rest into one "+N more" badge. Widths are
	// measured off an invisible copy of the same badges, so hiding the overflow
	// does not change what the next measurement sees.
	let row = $state<HTMLElement | null>(null);
	let ruler = $state<HTMLElement | null>(null);
	let triggerEl = $state<HTMLElement | null>(null);
	let moreEl = $state<HTMLElement | null>(null);
	let rowWidth = $state(0);
	let rulerWidth = $state(0);
	let fit = $state(Number.POSITIVE_INFINITY);
	const shown = $derived(active.slice(0, fit));
	const folded = $derived(active.length - shown.length);
	const moreLabel = $derived(t('chat-render-tools-more', { count: folded }));

	$effect(() => {
		if (!row || !ruler || rulerWidth === 0) return;
		const widths = [...ruler.querySelectorAll<HTMLElement>('[data-measure-badge]')].map(
			(badge) => badge.getBoundingClientRect().width
		);
		if (widths.length !== active.length) return;
		const gap = Number.parseFloat(getComputedStyle(row).columnGap) || 0;
		fit = fittingCapabilities(
			widths,
			triggerEl?.getBoundingClientRect().width ?? 0,
			moreEl?.getBoundingClientRect().width ?? 0,
			rowWidth,
			gap
		);
	});

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

<div bind:this={row} bind:clientWidth={rowWidth} class="relative flex min-w-0 flex-1 flex-nowrap items-center gap-1.5">
	{#if showTrigger}
		<button bind:this={triggerEl} type="button" class="btn btn-ghost btn-sm gap-1 rounded-full" title={t('chat-render-tools-tooltip')} onclick={openPicker} aria-expanded={open}>
			<span aria-hidden="true">+</span> {triggerLabel ?? t('chat-render-tools-label')}
		</button>
	{/if}
	{#if showActive && active.length > 0}
		<button type="button" class="badge badge-outline gap-1 sm:hidden" title={t('chat-render-active-count-title')} onclick={openPicker}>⌁ {active.length}</button>
	{/if}
	{#each showActive ? shown : [] as capability (`${capability.kind}:${capability.key}`)}
		<button type="button" class="badge badge-outline hidden gap-1 sm:inline-flex" title={t('chat-render-unpin-title')} onclick={() => onset(capability, 'auto')}>
			{capability.title} <span class="opacity-60">×</span>
		</button>
	{/each}
	{#if showActive && folded > 0}
		<button type="button" class="badge badge-outline hidden gap-1 sm:inline-flex" title={moreLabel} aria-label={moreLabel} onclick={openPicker}>{moreLabel}</button>
	{/if}
	<!-- The invisible copy the fit is measured from: same badges, same gap,
	     never constrained by the row and never interactive. -->
	{#if showActive && active.length > 0}
		<div bind:this={ruler} bind:clientWidth={rulerWidth} aria-hidden="true" class="invisible pointer-events-none absolute start-0 top-0 flex items-center gap-1.5 whitespace-nowrap">
			{#each active as capability (`${capability.kind}:${capability.key}`)}
				<span data-measure-badge class="badge badge-outline gap-1">{capability.title} <span class="opacity-60">×</span></span>
			{/each}
			<span bind:this={moreEl} class="badge badge-outline gap-1">{t('chat-render-tools-more', { count: active.length })}</span>
		</div>
	{/if}

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
				<CapabilityBrowser items={capabilities} labelledby={dialogId} focusSearch filter={(capability) => stateFilter === 'all' || capability.state === stateFilter}>
					{#snippet toolbar()}
						<div role="tablist" class="tabs tabs-box tabs-sm max-w-full overflow-x-auto">
							{#each stateFilters as state (state)}
								<button type="button" role="tab" class="tab gap-1 whitespace-nowrap {stateFilter === state ? 'tab-active' : ''}" aria-selected={stateFilter === state} onclick={() => (stateFilter = state)}>
									{stateLabel(state)} <span class="badge badge-sm badge-ghost">{stateCount(state)}</span>
								</button>
							{/each}
						</div>
					{/snippet}
					{#snippet groupActions(rows)}
						<div class="flex flex-col gap-1 sm:items-end">
							<span class="text-xs font-medium text-base-content/60">{t('chat-render-tools-set-group')}</span>
							{@render segmented(rows)}
						</div>
					{/snippet}
					{#snippet control(capability)}{@render segmented([capability])}{/snippet}
				</CapabilityBrowser>

				<footer class="flex min-h-16 items-center gap-3 border-t border-base-300 px-4 sm:px-6">
					<p class="min-w-0 flex-1 truncate text-sm text-base-content/60">{t('chat-render-tools-summary', counts)}</p>
					<button type="button" class="btn" onclick={closePicker}>{t('chat-render-tools-done')}</button>
				</footer>
			{:else}
				<div class="flex flex-1 items-center justify-center p-6 text-sm">
					<p>{t('chat-render-no-tools-prefix')} <a class="link" href="/tools/integrations">{t('nav-integrations')}</a>{t('chat-render-no-tools-suffix')}</p>
				</div>
			{/if}
		</div>
	</dialog>
</div>
