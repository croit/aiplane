<script lang="ts">
	import { goto } from '$app/navigation';
	import { base } from '$app/paths';
	import { me } from '$lib/session.svelte';
	import { t } from '$lib/i18n.svelte';

	// The SPA's home is the chat surface, mirroring the legacy `/` (which
	// 303s into the latest conversation). The identity card moved to /chat's
	// header there; this page exists only to route.
	$effect(() => {
		if (me.loaded) goto(`${base}/chat`);
	});
</script>

<p class="mt-8 text-center text-base-content/60">
	<span class="loading loading-spinner loading-sm"></span>
	{t('chrome-opening-conversations')}
</p>
