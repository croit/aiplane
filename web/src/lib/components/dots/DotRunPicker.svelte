<script lang="ts">
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import { dotRunOptions } from '#lib/dot-background.js';
	import type { Dot } from '#lib/dot-state.js';
	import { t } from '#lib/i18n.svelte.js';

	let { dots, initialId, value = $bindable(''), onchange }: {
		dots: Dot[];
		initialId: string | null;
		value?: string;
		onchange: () => void;
	} = $props();
	const options = $derived(dotRunOptions(dots, initialId, {
		person: t('dot-background-person'), inactive: t('dot-background-inactive'),
		unavailable: (id) => t('dot-background-unavailable', { id })
	}));
	const selected = $derived(dots.find((dot) => dot.id === value));
</script>

<fieldset class="fieldset w-full min-w-0">
	<legend class="fieldset-legend">{t('dot-background-run-as')}</legend>
	<SearchableSelect {options} bind:value ariaLabel={t('dot-background-run-as')} class="w-full" {onchange} />
	{#if value}
		<p class="label whitespace-normal">{t('dot-background-policy')}</p>
		{#if selected}
			<a class="link link-hover text-sm" href={`/dots#dot-${encodeURIComponent(value)}`}>{t('dot-background-configure')}</a>
			{#if !selected.active}<p class="text-sm text-warning" role="status">{t('dot-background-inactive-help')}</p>{/if}
		{:else}<p class="text-sm text-warning" role="status">{t('dot-background-unavailable-help')}</p>{/if}
	{/if}
</fieldset>
