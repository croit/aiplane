<script lang="ts">
	import { goto } from '$app/navigation';
	import { base } from '$app/paths';
	import { api } from '$lib/api';
	import { t } from '$lib/i18n.svelte';
	import { me } from '$lib/session.svelte';
	import { refreshSidebar } from '$lib/sidebar.svelte';

	let error = $state<string | null>(null);
	let started = false;

	$effect(() => {
		if (!me.loaded || !me.value || started) return;
		started = true;
		api.chatLanding()
			.then(async ({ session }) => {
				await refreshSidebar();
				await goto(`${base}/chat/${session.id}`, { replaceState: true });
			})
			.catch((caught) => { error = String(caught); });
	});
</script>

{#if error}
	<div class="alert alert-error m-4"><span>{error}</span></div>
{:else}
	<p class="mt-8 flex items-center justify-center gap-2 text-center text-base-content/60">
		<span class="loading loading-spinner loading-sm"></span>
		{t('chrome-opening-conversations')}
	</p>
{/if}
