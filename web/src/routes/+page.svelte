<script lang="ts">
	import { me } from '$lib/session.svelte';
</script>

<h1 class="text-2xl font-semibold">Signed in</h1>

{#if !me.loaded}
	<p class="mt-4 flex items-center gap-2 text-base-content/60">
		<span class="loading loading-spinner loading-sm"></span> Loading identity…
	</p>
{:else if me.value}
	<div class="card border border-base-300 bg-base-200 mt-4">
		<div class="card-body">
			<h2 class="card-title text-lg">Identity <span class="badge badge-ghost badge-sm">GET /api/v0/me</span></h2>
			<dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
				<dt class="font-medium">Name</dt>
				<dd>{me.value.name ?? '—'}</dd>
				<dt class="font-medium">Email</dt>
				<dd>{me.value.email}</dd>
				<dt class="font-medium">User ID</dt>
				<dd class="font-mono text-xs">{me.value.id}</dd>
				<dt class="font-medium">Roles</dt>
				<dd>
					{#each me.value.role_ids as role (role)}
						<span class="badge badge-outline badge-sm">{role}</span>
					{:else}
						<span class="text-base-content/50">none</span>
					{/each}
				</dd>
				<dt class="font-medium">Allowed tools</dt>
				<dd>
					{#each me.value.allowed_tools as tool (tool.id)}
						<span class="badge badge-sm">{tool.name}</span>
					{:else}
						<span class="text-base-content/50">none</span>
					{/each}
				</dd>
			</dl>
		</div>
	</div>
{/if}
