<script lang="ts">
	import { page } from '$app/state';
	import { safeReturnTo } from '$lib/auth';
	import { t } from '$lib/i18n.svelte';
	import SourceLink from '$lib/components/SourceLink.svelte';

	const returnTo = $derived(safeReturnTo(page.url.searchParams.get('return_to')));
</script>

<svelte:head><title>{t('login-page-title')}</title></svelte:head>

<div class="card w-full max-w-md border border-base-300 mx-auto">
	<div class="card-body">
		<h2 class="card-title text-2xl">{t('login-heading')}</h2>
		<p class="text-base-content/70">{t('login-description')}</p>
		<form action="/auth/login" method="get" class="mt-2">
			{#if returnTo}<input type="hidden" name="return_to" value={returnTo} />{/if}
			<button type="submit" class="btn btn-primary btn-block">{t('login-continue-button')}</button>
		</form>
		<p class="mt-4 text-center text-xs text-base-content/45"><SourceLink login /></p>
	</div>
</div>
