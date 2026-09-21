<script lang="ts">
	/**
	 * The access picker for a resource's `allowed_groups` — pools, RAG
	 * collections, MCP connectors, and whatever carries an ACL next.
	 *
	 * It exists so "a group picker looks like this" is decided once: the three
	 * call sites were byte-identical and had already started to drift. The
	 * labels stay here rather than in `multi-select.ts`, which is deliberately
	 * import-free so it runs under `node --test`.
	 */
	import SearchableSelect from './SearchableSelect.svelte';
	import { multiSelectOptions } from '$lib/multi-select';
	import { t } from '$lib/i18n.svelte';

	let {
		groups,
		values = $bindable([]),
		label,
		size = 'sm',
		class: className = ''
	}: {
		groups: string[];
		values: string[];
		label: string;
		size?: 'xs' | 'sm' | 'md';
		class?: string;
	} = $props();

	let options = $derived(
		multiSelectOptions(
			groups.map((group) => ({ value: group, label: group })),
			{ unknownLabel: t('multi-select-unknown') },
			values
		)
	);
</script>

<SearchableSelect
	multiple
	bind:values
	{options}
	{size}
	class={className}
	ariaLabel={label}
	summary={{
		empty: t('multi-select-none'),
		counted: (count: number) => t('multi-select-count', { count })
	}}
/>
