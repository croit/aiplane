<script lang="ts" generics="T extends CapabilityItem">
	import type { Snippet } from 'svelte';
	import type { CapabilityItem } from '#lib/api.js';
	import { capabilityGroups, capabilityId, descriptionOf, editLink, searchCapabilities } from '#lib/capability-picker.js';
	import { t } from '#lib/i18n.svelte.js';
	import { toolCategoryLabel } from '#lib/tools.js';
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';

	/**
	 * The searchable, grouped capability list the chat picker and the agent
	 * setup share: a group menu, a search over titles and descriptions, and
	 * one row per resource with its own title and description (or, for who
	 * may add one, "no description" and its edit page). The caller brings the
	 * control of each row (`control`), anything to show under it (`detail`),
	 * which rows a filter keeps (`filter`, applied after search and group, so
	 * the group counts stay whole) and their order (`order`).
	 */
	let {
		items,
		labelledby,
		control,
		detail = null,
		toolbar = null,
		groupActions = null,
		filter = () => true,
		order = (rows) => rows,
		groupLabel = toolCategoryLabel,
		searchPlaceholder = t('chat-render-tools-search-placeholder'),
		focusSearch = false
	}: {
		items: T[];
		labelledby: string;
		control: Snippet<[T]>;
		detail?: Snippet<[T]> | null;
		toolbar?: Snippet | null;
		groupActions?: Snippet<[T[]]> | null;
		filter?: (item: T) => boolean;
		order?: (rows: T[]) => T[];
		groupLabel?: (group: string) => string;
		searchPlaceholder?: string;
		focusSearch?: boolean;
	} = $props();

	let query = $state('');
	let selectedGroup = $state('');
	const groups = $derived(capabilityGroups(items));
	const searching = $derived(query.trim() !== '');
	const shown = $derived(
		order(searchCapabilities(items, query).filter((item) => (searching || !selectedGroup || item.group === selectedGroup) && filter(item)))
	);
	const groupOptions = $derived([
		{ value: '', label: t('chat-render-all-label'), description: t('chat-render-entry-count', { count: items.length }) },
		...groups.map((group) => ({ value: group.name, label: groupLabel(group.name), description: t('chat-render-entry-count', { count: group.rows.length }) }))
	]);

	function pick(group: string) {
		selectedGroup = group;
		query = '';
	}
</script>

<div class="flex flex-col gap-3 border-b border-base-300 px-4 py-3 sm:px-6 lg:flex-row lg:items-center">
	<label class="input w-full lg:max-w-xl">
		<span aria-hidden="true">⌕</span>
		<!-- A dialog hands focus to its first focusable element; `focusSearch`
		     makes the search field that element instead of the close button. -->
		<!-- svelte-ignore a11y_autofocus -->
		<input bind:value={query} placeholder={searchPlaceholder} aria-label={searchPlaceholder} autofocus={focusSearch} />
	</label>
	{@render toolbar?.()}
</div>

<div class="grid min-h-0 flex-1 md:grid-cols-[16rem_minmax(0,1fr)]">
	<nav class="hidden overflow-y-auto border-r border-base-300 bg-base-200/25 p-3 md:block" aria-label={t('chat-render-tools-category-label')}>
		<ul class="menu w-full gap-1">
			<li><button type="button" class={selectedGroup === '' ? 'menu-active' : ''} onclick={() => pick('')}><span class="min-w-0 flex-1 truncate">{t('chat-render-all-label')}</span><span class="badge badge-sm">{items.length}</span></button></li>
			{#each groups as group (group.name)}
				<li><button type="button" class={selectedGroup === group.name ? 'menu-active' : ''} onclick={() => pick(group.name)}><span class="min-w-0 flex-1 truncate">{groupLabel(group.name)}</span><span class="badge badge-sm">{group.rows.length}</span></button></li>
			{/each}
		</ul>
	</nav>

	<section class="flex min-h-0 min-w-0 flex-col" aria-labelledby={labelledby}>
		<div class="border-b border-base-300 p-3 md:hidden">
			<SearchableSelect options={groupOptions} bind:value={selectedGroup} onchange={() => (query = '')} ariaLabel={t('chat-render-tools-category-label')} class="w-full" />
		</div>
		<div class="flex flex-col gap-3 border-b border-base-300 px-4 py-3 sm:flex-row sm:items-center sm:px-6">
			<div class="min-w-0 flex-1">
				<h3 class="truncate text-lg font-semibold">{searching ? t('chat-render-tools-search-results') : selectedGroup ? groupLabel(selectedGroup) : t('chat-render-all-label')}</h3>
				<p class="text-sm text-base-content/60">{t('chat-render-entry-count', { count: shown.length })}</p>
			</div>
			{#if groupActions && !searching && shown.length > 0}{@render groupActions(shown)}{/if}
		</div>

		<div class="min-h-0 flex-1 overflow-y-auto px-4 sm:px-6">
			{#if shown.length > 0}
				<ul class="divide-y divide-base-300/60">
					{#each shown as item (capabilityId(item))}
						{@const about = descriptionOf(item)}
						{@const link = editLink(item)}
						<li class="flex flex-col gap-3 py-4 sm:flex-row sm:items-center">
							<div class="min-w-0 flex-1">
								<h4 class="font-medium">{item.title}</h4>
								{#if about && 'text' in about}
									<p class="mt-1 max-w-3xl text-sm text-base-content/60">{about.text}</p>
								{:else if about}
									<p class="mt-1 text-sm text-base-content/60">{t('tools-no-description')}</p>
								{/if}
								{#if link}<a class="link link-hover text-sm" href={link}>{t('tools-configure-link')}</a>{/if}
								{@render detail?.(item)}
							</div>
							{@render control(item)}
						</li>
					{/each}
				</ul>
			{:else}
				<div class="flex h-full min-h-48 items-center justify-center text-center text-base-content/60">{t('chat-render-tools-empty')}</div>
			{/if}
		</div>
	</section>
</div>
