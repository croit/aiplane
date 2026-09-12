<script lang="ts">
	import { base } from '$app/paths';
	import { t } from '$lib/i18n.svelte';

	// The catalog and the run history are two jobs, so they are two pages —
	// the old single page stacked both and neither could be linked to. These
	// links are the seam between them; `current` decides which is lit.
	let { current, failed = 0 }: { current: 'workflows' | 'jobs'; failed?: number } = $props();

	const tabs = [
		{ id: 'workflows', href: '/admin/comfyui', label: 'admin-comfyui-tab-workflows' },
		{ id: 'jobs', href: '/admin/comfyui/jobs', label: 'admin-comfyui-tab-jobs' }
	] as const;
</script>

<div role="tablist" class="tabs tabs-box mb-5 w-fit p-1">
	{#each tabs as tab (tab.id)}
		<a
			role="tab"
			href="{base}{tab.href}"
			aria-selected={current === tab.id}
			class="tab gap-2 {current === tab.id ? 'tab-active' : ''}"
		>
			{t(tab.label)}
			{#if tab.id === 'jobs' && failed > 0}<span class="badge badge-error badge-xs">{failed}</span>{/if}
		</a>
	{/each}
</div>
