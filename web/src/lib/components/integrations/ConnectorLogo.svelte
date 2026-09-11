<script lang="ts">
	import NavIcon from '$lib/components/NavIcon.svelte';

	let { connectorKey, icon }: { connectorKey: string; icon: string | null } = $props();

	const paths: Record<string, { color: string; path: string }> = {
		github: { color: 'currentColor', path: 'M12 .3a12 12 0 0 0-3.8 23.4c.6.1.8-.3.8-.6v-2.1c-3.3.7-4-1.6-4-1.6-.5-1.4-1.3-1.7-1.3-1.7-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1.1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.8-1.6-2.7-.3-5.5-1.3-5.5-5.9 0-1.3.5-2.4 1.2-3.2-.1-.3-.5-1.5.1-3.2 0 0 1-.3 3.3 1.2a11.4 11.4 0 0 1 6 0C17.7 4.7 18.7 5 18.7 5c.6 1.7.2 2.9.1 3.2.8.8 1.2 1.9 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.6c0 .3.2.7.8.6A12 12 0 0 0 12 .3Z' },
		gitlab: { color: '#FC6D26', path: 'm23.6 9.6-3.3-8.6a.9.9 0 0 0-1.6.1l-2.2 6.7h-9L5.3 1.1A.9.9 0 0 0 3.7 1L.4 9.5a6.1 6.1 0 0 0 2 7.1L12 23.8l9.6-7.2a6.1 6.1 0 0 0 2-7Z' },
		atlassian: { color: '#0052CC', path: 'M7.1 11.1a.7.7 0 0 0-1.1.1L.1 23a.7.7 0 0 0 .6 1h8.2a.7.7 0 0 0 .6-.4c1.8-3.7.7-9.2-2.4-12.5ZM11.4.4a15.5 15.5 0 0 0-.9 15.3l4 7.9a.7.7 0 0 0 .6.4h8.2a.7.7 0 0 0 .6-1L12.6.4a.7.7 0 0 0-1.2 0Z' },
		discord: { color: '#5865F2', path: 'M20.3 4.4a19.8 19.8 0 0 0-4.9-1.5c-.2.4-.4.9-.6 1.2a19 19 0 0 0-5.5 0c-.2-.4-.4-.9-.6-1.2a19.7 19.7 0 0 0-4.9 1.5C.5 9-.3 13.6.1 18.1c2.1 1.5 4 2.4 6 3 .5-.6.9-1.3 1.2-2-.6-.2-1.2-.5-1.8-.9l.4-.3a13.8 13.8 0 0 0 12.1 0l.4.3c-.6.4-1.2.7-1.9.9.4.7.8 1.4 1.3 2 2-.6 3.9-1.5 6-3 .5-5.2-.8-9.7-3.5-13.7ZM8 15.3c-1.2 0-2.2-1.1-2.2-2.4s1-2.4 2.2-2.4 2.2 1.1 2.2 2.4-1 2.4-2.2 2.4Zm8 0c-1.2 0-2.2-1.1-2.2-2.4s1-2.4 2.2-2.4 2.2 1.1 2.2 2.4-1 2.4-2.2 2.4Z' },
		google_workspace: { color: '#4285F4', path: 'M12.5 10.9v3.3h7.8c-.2 1.8-.8 3.2-1.8 4.1-1.1 1.2-2.9 2.4-6 2.4a8.7 8.7 0 1 1 0-17.4c2.6 0 4.5 1 5.9 2.3l2.3-2.3A11.8 11.8 0 0 0 12.5 0a12 12 0 1 0 0 24c3.6 0 6.3-1.2 8.4-3.4 2.2-2.2 2.8-5.2 2.8-7.6l-.2-2.1h-11Z' }
	};
</script>

{#if paths[connectorKey]}
	<svg viewBox="0 0 24 24" width="26" height="26" fill={paths[connectorKey].color} class="inline-block shrink-0" aria-hidden="true"><path d={paths[connectorKey].path}></path></svg>
{:else if connectorKey === 'kiwi'}
	<svg viewBox="0 0 24 24" width="26" height="26" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><path d="M22 2 9 15M22 2l-7 20-4-9-9-4Z"></path></svg>
{:else if icon}
	<span class="text-2xl leading-none">{icon}</span>
{:else}
	<NavIcon name="plug" size={26} />
{/if}
