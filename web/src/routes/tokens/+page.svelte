<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError } from '$lib/api';
	import type { TokenSummary } from '$lib/api';

	let tokens = $state<TokenSummary[]>([]);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let busy = $state(false);

	// The one-time minted secret. Kept in its own piece of state because it
	// exists exactly once — re-fetching the list can never bring it back.
	let minted = $state<{ name: string; plaintext: string } | null>(null);

	let name = $state('');
	let ttlDays = $state(90);

	async function refresh() {
		try {
			tokens = await api.listTokens();
		} catch (err) {
			error = String(err);
		}
	}

	async function create() {
		const trimmed = name.trim();
		if (!trimmed || busy) return;
		busy = true;
		notice = null;
		try {
			const res = await api.createToken({
				name: trimmed,
				ttl_days: ttlDays,
				tools_enabled: false,
				disabled_tools: []
			});
			minted = { name: res.token.name, plaintext: res.plaintext };
			name = '';
			await refresh();
		} catch (err) {
			notice = String(err);
		} finally {
			busy = false;
		}
	}

	async function revoke(id: string) {
		if (!confirm('Revoke this token? Clients using it stop working immediately.')) return;
		try {
			await api.revokeToken(id);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function rotate(id: string) {
		if (!confirm('Issue a new secret? The old one stops working immediately.')) return;
		try {
			const res = await api.rotateToken(id);
			minted = { name: res.token.name, plaintext: res.plaintext };
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function remove(id: string) {
		if (!confirm('Delete this token row for good?')) return;
		try {
			await api.deleteToken(id);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function toggleTools(token: TokenSummary) {
		try {
			await api.updateTokenTools(token.id, {
				tools_enabled: !token.tools_enabled,
				disabled_tools: token.disabled_tools
			});
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	function copyPlaintext() {
		if (minted) void navigator.clipboard?.writeText(minted.plaintext);
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">API tokens</h1>
</div>

<p class="text-base-content/60 text-sm mb-6">
	Bearer tokens for the OpenAI-compatible API. The plaintext is shown only at creation time — store it somewhere safe.
</p>

{#if error}
	<div class="alert alert-error mb-4"><span>{error}</span></div>
{/if}
{#if notice}
	<div class="alert alert-warning mb-4"><span>{notice}</span></div>
{/if}

{#if minted}
	<div class="card border border-success mb-6">
		<div class="card-body">
			<h2 class="card-title text-base">
				<span class="text-success">✓</span> Token created — copy it now
			</h2>
			<p class="text-sm text-base-content/70">You won't be able to see this value again.</p>
			<div class="relative">
				<pre class="bg-base-100 border border-base-300 rounded-md p-3 pr-12 m-0 font-mono text-xs select-all break-all whitespace-pre-wrap">{minted.plaintext}</pre>
				<button
					class="btn btn-ghost btn-sm btn-square absolute top-1.5 right-1.5"
					onclick={copyPlaintext}
					aria-label="Copy token"
				>
					Copy
				</button>
			</div>
			<p class="text-xs text-base-content/60 mt-2">Name: {minted.name}</p>
		</div>
	</div>
{/if}

<div class="card border border-base-300 mb-6">
	<div class="card-body">
		<h2 class="card-title">Create token</h2>
		<div class="flex flex-wrap items-end gap-3">
			<label class="flex flex-col gap-1 flex-1 min-w-48">
				<span class="label-text">Name</span>
				<input
					class="input input-bordered w-full"
					placeholder="e.g. laptop, ci-runner"
					bind:value={name}
					onkeydown={(e) => e.key === 'Enter' && create()}
				/>
			</label>
			<label class="flex flex-col gap-1 w-32">
				<span class="label-text">TTL (days)</span>
				<input class="input input-bordered w-full" type="number" min="1" max="1825" bind:value={ttlDays} />
			</label>
			<button class="btn btn-primary" onclick={create} disabled={!name.trim() || busy}>
				Create token
			</button>
		</div>
	</div>
</div>

<div class="card border border-base-300">
	<div class="card-body">
		<h2 class="card-title">Your tokens</h2>
		{#if tokens.length === 0}
			<p class="text-base-content/60 text-sm">No tokens yet. Create one above.</p>
		{:else}
			<ul class="flex flex-col divide-y divide-base-300">
				{#each tokens as token (token.id)}
					<li class="py-3">
						<div class="flex items-center gap-4">
							<div class="flex-1 min-w-0">
								<div class="text-sm font-medium">{token.name}</div>
								<div class="text-xs text-base-content/60">
									created {new Date(token.created_at).toLocaleDateString()} · expires{' '}
									{new Date(token.expires_at).toLocaleDateString()}
								</div>
							</div>
							{#if token.revoked}
								<span class="badge badge-error">revoked</span>
							{:else}
								<span class="badge badge-secondary">active</span>
							{/if}
							<label class="flex items-center gap-2 text-sm">
								<input
									type="checkbox"
									class="toggle toggle-primary toggle-sm"
									checked={token.tools_enabled}
									disabled={token.revoked}
									onchange={() => toggleTools(token)}
								/>
								<span class="text-base-content/60">Tool use</span>
							</label>
							{#if !token.revoked}
								<button class="btn btn-outline btn-sm" onclick={() => rotate(token.id)}>Rotate</button>
								<button class="btn btn-error btn-sm" onclick={() => revoke(token.id)}>Revoke</button>
							{:else}
								<button class="btn btn-outline btn-sm" onclick={() => remove(token.id)}>Remove</button>
							{/if}
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
