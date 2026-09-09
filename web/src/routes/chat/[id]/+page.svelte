<script lang="ts">
	import Conversation from '$lib/Conversation.svelte';

	let { data } = $props<{ data: { id: string } }>();
</script>

<!--
	Keyed, so switching conversations remounts rather than mutates.

	SvelteKit reuses a component across a param-only navigation, so without
	this the message list, composer and live SSE stream would stay attached to
	the previous conversation while the URL and the sidebar highlight moved on.
	Keying makes every piece of per-conversation state reset structurally —
	the alternative is a hand-maintained list of things to clear, which is how
	the reasoning-effort picker came to carry the previous chat's value.
-->
{#key data.id}
	<Conversation id={data.id} />
{/key}
