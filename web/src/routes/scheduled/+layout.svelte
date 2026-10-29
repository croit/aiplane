<script lang="ts">
	import type { Snippet } from 'svelte';
	import { page } from '$app/state';

	let { children }: { children: Snippet } = $props();
</script>

{#key JSON.stringify([page.url.pathname, page.url.searchParams.get('principal_id'), page.url.searchParams.get('personal_agent_id')])}
	{@render children()}
{/key}
