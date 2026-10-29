<script lang="ts">
	import type { Snippet } from 'svelte';
	import { page } from '$app/state';
	import AgentShell from '#lib/components/agents/AgentShell.svelte';
	import PersonalAgentEditor from '#lib/components/dots/PersonalAgentEditor.svelte';
	import { dotsApi, type DotProfile } from '#lib/dots.js';
	import { ApiError } from '#lib/api.js';

	let { children }: { children: Snippet } = $props();
	const id = $derived(page.params.id ?? '');
	// Asked once per agent: a query change (?tab=, ?test_session=) must not
	// remount the editor and lose its state.
	const lookup = $derived(personal(id));

	// A personal agent answers its profile to its owner and to admins; every
	// other agent answers 404 there and opens in the agent builder.
	async function personal(id: string): Promise<DotProfile | null> {
		try {
			return await dotsApi.profile(id);
		} catch (error) {
			if (error instanceof ApiError && error.status === 404) return null;
			throw error;
		}
	}
</script>

{#key id}
	{#await lookup}
		<div class="skeleton h-96 w-full"></div>
	{:then profile}
		{#if profile}
			<PersonalAgentEditor initial={profile} />
		{:else}
			<AgentShell {id}>{@render children()}</AgentShell>
		{/if}
	{:catch error}
		<div class="alert alert-error" role="alert"><span>{error.message}</span></div>
	{/await}
{/key}
