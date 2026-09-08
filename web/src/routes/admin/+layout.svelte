<script lang="ts">
	let { children } = $props<{ children: import('svelte').Snippet }>();
	import { base } from '$app/paths';
	import { me } from '$lib/session.svelte';

	const links: [string, string][] = [
		['Groups', '/admin/groups'],
		['Users', '/admin/users'],
		['Models', '/admin/models'],
		['Limits', '/admin/limits'],
		['Settings', '/admin/settings'],
		['Tokens', '/admin/tokens'],
		['Upstreams', '/admin/upstreams'],
		['Skills', '/admin/skills'],
		['Connectors', '/admin/connectors'],
		['ComfyUI', '/admin/comfyui']
	];
</script>

<div class="flex flex-wrap items-center gap-2 mb-6">
	<h1 class="text-2xl font-bold mr-4">Admin</h1>
	{#each links as [label, path] (path)}
		<a href="{base}{path}" class="btn btn-ghost btn-sm">{label}</a>
	{/each}
</div>

{#if !me.value?.role_ids?.includes('admin')}
	<div class="alert alert-warning mb-4"><span>These pages need the admin role.</span></div>
{/if}

{@render children()}
