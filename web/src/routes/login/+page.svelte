<script lang="ts">
	// The landing point for a signed-out user. The layout's 401 effect
	// deliberately leaves this route alone, so this page can start the
	// OIDC dance itself — it bounces straight to the IdP, so under normal
	// flow there is no visible stop here; it exists for direct visits and
	// for the redirect to land on if sign-out keeps the user in the SPA.
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { loginUrl } from '$lib/api';

	onMount(() => {
		const back = page.url.searchParams.get('return_to') ?? '/app';
		window.location.href = loginUrl(back);
	});
</script>

<p class="mt-8 text-center text-base-content/60">Redirecting to sign-in…</p>
