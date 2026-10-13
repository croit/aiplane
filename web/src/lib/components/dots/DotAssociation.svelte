<script lang="ts">
	import type { DotBinding } from '#lib/dot-background.js';
	import { t } from '#lib/i18n.svelte.js';
	let { binding }: { binding: DotBinding } = $props();
</script>

{#if binding.dot_id}
	<div class="mt-2 flex flex-wrap items-center gap-2 text-sm">
		{#if binding.dot}
			<a class="link link-hover" href={`/dots#dot-${encodeURIComponent(binding.dot_id)}`}>{t('dot-background-association', { name: binding.dot.display || binding.dot.name })}</a>
			{#if !binding.dot.active}<span class="badge badge-warning badge-sm">{t('dots-status-inactive')}</span>{/if}
		{:else}<span class="badge badge-warning badge-sm break-all whitespace-normal">{t('dot-background-unavailable', { id: binding.dot_id })}</span>{/if}
	</div>
{/if}
