<script lang="ts">
	import { LOCALES, LOCALE_NAMES, locale, setLocale, t } from '$lib/i18n.svelte';

	let { placement = 'up' }: { placement?: 'up' | 'down' } = $props();
	let open = $state(false);
</script>

<div class="relative">
	<button class="btn btn-ghost btn-xs" onclick={() => (open = !open)} aria-label={t('chrome-lang-switcher-aria')} title={t('chrome-lang-switcher-aria')}>
		<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><circle cx="12" cy="12" r="10"/><path d="M2 12h20"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1 4-10 15.3 15.3 0 0 1 4-10z"/></svg>
	</button>
	{#if open}
		<button class="fixed inset-0 z-40 cursor-default bg-transparent" aria-label={t('nav-close-menu')} onclick={() => (open = false)}></button>
		<ul class="menu absolute right-0 z-50 w-36 rounded-box border border-base-300 bg-base-200 p-1 shadow {placement === 'up' ? 'bottom-9' : 'top-9'}">
			{#each LOCALES as code (code)}<li><button class:font-semibold={locale.current === code} class="text-sm" onclick={() => { void setLocale(code); open = false; }}>{LOCALE_NAMES[code]}</button></li>{/each}
		</ul>
	{/if}
</div>
