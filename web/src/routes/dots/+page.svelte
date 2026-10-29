<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';

	// Personal agents live in the agent list; a bookmarked /dots or
	// /dots?dot_id=… lands where that agent is now.
	onMount(() => {
		const id = page.url.searchParams.get('dot_id');
		void goto(id ? `/agents/${encodeURIComponent(id)}` : '/agents', { replace: true });
	});
</script>
