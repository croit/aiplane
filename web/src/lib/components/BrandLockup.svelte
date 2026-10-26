<script lang="ts">
	import { t } from '#lib/i18n.svelte.js';

	// The operator's uploaded logo, if there is one. A missing slot is a 404
	// the onerror handler walks back to the built-in croit mark — the same
	// two-image light/dark structure as before, only the first source.
	let lightCustom = $state(false);
	let darkCustom = $state(false);
</script>

<span class="inline-flex items-center gap-3">
	<img
		class="h-5 w-auto dark:hidden"
		src={lightCustom ? '/croit-logo-black.svg' : '/brand/logo'}
		alt=""
		aria-hidden="true"
		onerror={() => (lightCustom = true)}
	/>
	<img
		class="hidden h-5 w-auto dark:block"
		src={darkCustom ? '/croit-logo-white.svg' : '/brand/logo-dark'}
		alt=""
		aria-hidden="true"
		onerror={() => (darkCustom = true)}
	/>
	<span class="border-l border-base-300 pl-3 text-sm font-semibold tracking-wide">{t('nav-brand')}</span>
</span>
