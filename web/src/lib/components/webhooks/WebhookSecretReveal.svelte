<script lang="ts">
	import { t } from '$lib/i18n.svelte';
	let { url } = $props<{ url: string }>();
</script>

<div class="alert alert-success mb-6 flex-col items-start gap-2">
	<div class="flex items-center gap-2 font-bold">✓ {t('webhooks-reveal-heading')}</div>
	<div class="text-sm opacity-80">{t('webhooks-reveal-note')}</div>
	<div class="flex w-full min-w-0 items-center gap-2"><input class="input input-sm min-w-0 w-full font-mono text-xs" type="text" readonly value={url} aria-label={t('webhooks-reveal-heading')} /><button class="btn btn-ghost btn-square btn-sm" type="button" title={t('webhooks-copy')} aria-label={t('webhooks-copy')} onclick={() => navigator.clipboard?.writeText(url)}>⧉</button></div>
</div>
