<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost } from '$lib/admin-client';

	interface Connector {
		key: string;
		title: string;
		description: string | null;
		auth_type: string;
		connected: boolean;
	}
	let connectors = $state<Connector[]>([]);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let tokenFor = $state<string | null>(null);
	let token = $state('');

	async function refresh() {
		try {
			const data = await adminJson<{ connectors: Connector[] }>('/api/v0/integrations');
			connectors = data.connectors;
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function connect(key: string) {
		try {
			await adminPost(`/api/v0/integrations/${encodeURIComponent(key)}/token`, { token });
			notice = `Connected ${key}.`;
			tokenFor = null;
			token = '';
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function disconnect(key: string) {
		if (!confirm(`Disconnect ${key}?`)) return;
		try {
			await adminPost(`/api/v0/integrations/${encodeURIComponent(key)}/disconnect`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">Integrations</h1>
</div>

<p class="text-base-content/60 text-sm mb-6">
	Connectors feed extra tools into your conversations. OAuth connectors connect through the legacy flow during migration; static-token ones connect here.
</p>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

<ul class="flex flex-col gap-2">
	{#each connectors as c (c.key)}
		<li class="card border border-base-300">
			<div class="card-body py-3 flex-row items-center gap-3 flex-wrap">
				{#if c.connected}<span class="badge badge-success badge-sm">connected</span>{:else}<span class="badge badge-ghost badge-sm">not connected</span>{/if}
				<span class="font-medium">{c.title}</span>
				<span class="text-xs text-base-content/60 flex-1 min-w-32">{c.description}</span>
				{#if c.auth_type === 'static_bearer'}
					{#if tokenFor === c.key}
						<div class="join">
							<input class="input input-bordered input-sm join-item w-44" type="password" placeholder="token" bind:value={token} />
							<button class="btn btn-primary btn-sm join-item" onclick={() => connect(c.key)}>Connect</button>
						</div>
					{:else if !c.connected}
						<button class="btn btn-outline btn-xs" onclick={() => (tokenFor = c.key)}>Connect</button>
					{/if}
				{/if}
				{#if c.connected}
					<button class="btn btn-ghost btn-xs text-error" onclick={() => disconnect(c.key)}>Disconnect</button>
				{/if}
			</div>
		</li>
	{/each}
</ul>
