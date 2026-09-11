<script lang="ts">
	let { email, name, id }: { email: string; name: string | null; id: string } = $props();
</script>

<div class="font-medium">{email}</div>
{#if name}<div class="text-xs text-base-content/60">{name}</div>{/if}
<div class="break-all font-mono text-xs text-base-content/40">{id}</div>
