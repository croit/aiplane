<script lang="ts">
	import { tick } from 'svelte';
	import { t } from '$lib/i18n.svelte';
	import { filterSearchOptions, nextEnabledOptionIndex, type SearchOption } from '$lib/searchable-select';

	const generatedId = $props.id();
	let {
		id = generatedId,
		options,
		value = $bindable(''),
		ariaLabel,
		placeholder = '',
		size = 'md',
		class: className = '',
		disabled = false,
		onchange
	}: {
		id?: string;
		options: SearchOption[];
		value: string;
		ariaLabel: string;
		placeholder?: string;
		size?: 'xs' | 'sm' | 'md';
		class?: string;
		disabled?: boolean;
		onchange?: (value: string) => void;
	} = $props();

	let root = $state<HTMLDivElement>();
	let trigger = $state<HTMLButtonElement>();
	let searchInput = $state<HTMLInputElement>();
	let listbox = $state<HTMLUListElement>();
	let open = $state(false);
	let query = $state('');
	let activeIndex = $state(0);
	let filtered = $derived(filterSearchOptions(options, query));
	let selected = $derived(options.find((option) => option.value === value));
	let buttonSize = $derived(size === 'xs' ? 'btn-xs' : size === 'sm' ? 'btn-sm' : 'btn-md');

	/* Where the popup sits, in viewport coordinates. It is NOT laid out inside
	 * the trigger's own box: an absolutely positioned panel is clipped by any
	 * scrolling ancestor, and this select lives inside plenty of them — the
	 * document canvas (`overflow-y-auto`, and a panel narrower than the popup),
	 * a `modal-box`, an admin card. In the canvas that clipping cut the option
	 * rows off at the panel edge: what was left looked like an empty box and
	 * every click in it landed on the chat behind. So the popup goes into the
	 * browser's top layer (`popover`) and is placed against the viewport here,
	 * clamped so it can never hang off-screen. */
	type Placement = { left: number; top: number | null; bottom: number | null; width: number; maxHeight: number };
	const gap = 4;
	const edge = 8;
	const roomy = 672;
	const shortest = 160;
	const tallest = 360;
	let placement = $state<Placement>({ left: 0, top: 0, bottom: null, width: 0, maxHeight: tallest });

	const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), Math.max(low, high));

	function place() {
		const anchor = trigger?.getBoundingClientRect();
		if (!anchor) return;
		const viewport = { width: window.innerWidth, height: window.innerHeight };
		const width = clamp(anchor.width, Math.min(roomy, viewport.width - edge * 2), viewport.width - edge * 2);
		// Right-aligned with the trigger, then pulled back inside the viewport.
		const left = clamp(anchor.right - width, edge, viewport.width - edge - width);
		const below = viewport.height - anchor.bottom - gap - edge;
		const above = anchor.top - gap - edge;
		const drop = below >= tallest || below >= above;
		const space = drop ? below : above;
		const maxHeight = Math.min(clamp(space, shortest, tallest), viewport.height - edge * 2);
		// In a viewport too short for even the smallest panel, overlapping the
		// trigger beats hanging off the edge where nothing can reach it.
		const offset = clamp(drop ? anchor.bottom + gap : viewport.height - anchor.top + gap, edge, viewport.height - edge - maxHeight);
		placement = drop
			? { left, top: offset, bottom: null, width, maxHeight }
			: { left, top: null, bottom: offset, width, maxHeight };
	}

	/* The top layer does not follow the trigger, so re-place it whenever
	 * anything that moved the trigger happens while the popup is open. */
	$effect(() => {
		if (!open) return;
		const follow = () => place();
		window.addEventListener('resize', follow);
		window.addEventListener('scroll', follow, true);
		return () => {
			window.removeEventListener('resize', follow);
			window.removeEventListener('scroll', follow, true);
		};
	});

	/* Raise the panel into the top layer, above every stacking context and
	 * outside every scroll clip. Older browsers without the popover API fall
	 * back to the same fixed coordinates, which already escape overflow. */
	function raise(node: HTMLElement) {
		try {
			node.showPopover();
		} catch {
			/* no popover support — the fixed placement stands on its own */
		}
	}

	async function show() {
		if (disabled) return;
		place();
		open = true;
		query = '';
		const selectedIndex = options.findIndex((option) => option.value === value && !option.disabled);
		activeIndex = selectedIndex >= 0 ? selectedIndex : nextEnabledOptionIndex(options, -1, 1);
		await tick();
		searchInput?.focus();
		revealActive();
	}

	function hide() {
		open = false;
		query = '';
	}

	function choose(option: SearchOption) {
		if (option.disabled) return;
		value = option.value;
		onchange?.(option.value);
		hide();
		void tick().then(() => trigger?.focus());
	}

	function search(event: Event) {
		query = (event.currentTarget as HTMLInputElement).value;
		const next = filterSearchOptions(options, query);
		activeIndex = next.findIndex((option) => !option.disabled);
	}

	function revealActive() {
		listbox?.querySelector(`[data-option-index="${activeIndex}"]`)?.scrollIntoView({ block: 'nearest' });
	}

	function onSearchKeydown(event: KeyboardEvent) {
		if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
			event.preventDefault();
			activeIndex = nextEnabledOptionIndex(filtered, activeIndex, event.key === 'ArrowDown' ? 1 : -1);
			void tick().then(revealActive);
		} else if (event.key === 'Enter' && activeIndex >= 0) {
			event.preventDefault();
			const option = filtered[activeIndex];
			if (option) choose(option);
		} else if (event.key === 'Escape') {
			event.preventDefault();
			hide();
			void tick().then(() => trigger?.focus());
		}
	}

	function closeFromOutside(event: MouseEvent) {
		if (open && !root?.contains(event.target as Node)) hide();
	}
</script>

<svelte:window onclick={closeFromOutside} />

<div bind:this={root} class="relative min-w-0 {className}">
	<button
		bind:this={trigger}
		type="button"
		class="btn {buttonSize} w-full min-w-0 justify-between gap-2 border-base-300 bg-base-200 font-normal focus-visible:outline-1 focus-visible:outline-info focus-visible:outline-offset-1"
		role="combobox"
		aria-label={ariaLabel}
		aria-haspopup="listbox"
		id={id}
		aria-controls={`${id}-listbox`}
		aria-expanded={open}
		disabled={disabled}
		onclick={() => (open ? hide() : void show())}
	>
		<span class="truncate">{selected?.label ?? (placeholder || value)}</span>
		<svg class="size-4 shrink-0 opacity-60" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg>
	</button>

	{#if open}
		<div
			use:raise
			popover="manual"
			class="fixed z-50 m-0 flex flex-col overflow-hidden rounded-box border border-base-300 bg-base-100 p-2 text-base-content shadow-xl"
			style:left="{placement.left}px"
			style:top={placement.top === null ? 'auto' : `${placement.top}px`}
			style:bottom={placement.bottom === null ? 'auto' : `${placement.bottom}px`}
			style:right="auto"
			style:width="{placement.width}px"
			style:max-height="{placement.maxHeight}px"
		>
			<label class="input input-sm flex w-full shrink-0 items-center gap-2 focus-within:outline-1 focus-within:outline-info focus-within:outline-offset-1">
				<svg class="size-4 shrink-0 opacity-50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-4-4" /></svg>
				<input bind:this={searchInput} class="min-w-0 flex-1" value={query} oninput={search} onkeydown={onSearchKeydown} placeholder={t('searchable-select-search-placeholder')} aria-label={t('searchable-select-search-aria', { field: ariaLabel })} />
				{#if query}<button type="button" class="btn btn-ghost btn-xs btn-circle" onclick={() => { query = ''; activeIndex = nextEnabledOptionIndex(options, -1, 1); void tick().then(() => searchInput?.focus()); }} aria-label={t('searchable-select-clear-search')}>✕</button>{/if}
			</label>

			{#if filtered.length > 0}
				<ul bind:this={listbox} id={`${id}-listbox`} class="mt-2 flex min-h-0 w-full flex-1 flex-col gap-1 overflow-y-auto p-0" role="listbox" aria-label={ariaLabel}>
					{#each filtered as option, index (option.value)}
						<li class="list-none">
							<button
								type="button"
								class="card card-xs w-full border text-left transition-colors {index === activeIndex ? 'border-base-300 bg-base-200' : 'border-transparent bg-base-100'} {option.value === value ? 'font-semibold' : ''}"
								role="option"
								aria-selected={option.value === value}
								data-option-index={index}
								disabled={option.disabled}
								onmouseenter={() => { if (!option.disabled) activeIndex = index; }}
								onclick={() => choose(option)}
							>
								<span class="grid w-full min-w-0 grid-cols-[1rem_minmax(0,1fr)] items-start gap-2 px-3 py-2">
									<span class="flex h-5 w-4 items-center justify-center">{option.value === value ? '✓' : ''}</span>
									<span class="min-w-0 flex-1">
										<span class="flex min-w-0 items-center justify-between gap-x-3">
											<span class="truncate text-sm leading-5" title={option.label}>{option.label}</span>
											{#if option.badges?.length}
												<span class="ml-auto flex shrink-0 justify-end gap-1">
													{#each option.badges as badge}
														{#if badge.tone === 'success'}
															<span class="badge badge-success badge-xs gap-0.5 font-normal"><span aria-hidden="true">✓</span>{badge.label}</span>
														{:else}
															<span class="badge badge-error badge-xs gap-0.5 font-normal line-through"><span aria-hidden="true">✕</span>{badge.label}</span>
														{/if}
													{/each}
												</span>
											{/if}
										</span>
										{#if option.description}<span class="mt-0.5 block text-xs font-normal text-base-content/60">{option.description}</span>{/if}
									</span>
								</span>
							</button>
						</li>
					{/each}
				</ul>
			{:else}
				<div class="px-3 py-6 text-center text-sm text-base-content/60">{t('searchable-select-no-results')}</div>
			{/if}
		</div>
	{/if}
</div>
