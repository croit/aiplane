<script lang="ts">
	let { children } = $props<{ children: import('svelte').Snippet }>();
	import { base } from '$app/paths';
	import { me } from '$lib/session.svelte';


</script>

{#if !me.value?.role_ids?.includes('admin')}
	<div class="alert alert-warning mb-4"><span>These pages need the admin role.</span></div>
{/if}

{@render children()}
