<script lang="ts">
	import { t } from '#lib/i18n.svelte.js';
	let { title, intro = null, backHref = '/webhooks' }: { title: string; intro?: string | null; backHref?: string } = $props();
</script>

<div class="mb-4 flex items-center gap-3"><a class="btn btn-ghost btn-sm" href={backHref}>← {t('webhooks-back')}</a><h1 class="text-2xl font-bold">{title}</h1></div>
{#if intro}<p class="mb-6 text-sm text-base-content/60">{intro}</p>{/if}
