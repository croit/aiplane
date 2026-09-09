<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError } from '$lib/api';
	import type { TokenSummary } from '$lib/api';
	import { adminPut, adminPost } from '$lib/admin-client';

	let tokens = $state<TokenSummary[]>([]);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let busy = $state(false);

	// The one-time minted secret. Kept in its own piece of state because it
	// exists exactly once — re-fetching the list can never bring it back.
	let minted = $state<{ name: string; plaintext: string } | null>(null);

	let name = $state('');
	let ttlDays = $state(90);
	// Per-token panel state: which token's panel is expanded, its model
	// allowlist draft, and its quota draft.
	let expanded = $state<string | null>(null);
	let modelsInput = $state('');
	let models = $state<string[]>([]);
	let restrict = $state(false);
	let quotaDimension = $state('requests');
	let quotaWindow = $state('day');
	let quotaValue = $state('');
	let allModels = $state<string[]>([]);
	// Per-token MCP "ask" policy. Write-only on the server (there is no read
	// endpoint), so this mirrors what this page last set rather than claiming
	// to show stored state.
	let mcpAllow = $state<Record<string, boolean>>({});

	async function refresh() {
		try {
			tokens = await api.listTokens();
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	function togglePanel(t: TokenSummary) {
		if (expanded === t.id) {
			expanded = null;
			return;
		}
		expanded = t.id;
		quotaValue = '';
	}

	async function saveModels(t: TokenSummary) {
		notice = null;
		try {
			const list = modelsInput
				.split(',')
				.map((m) => m.trim())
				.filter(Boolean);
			await adminPut(`/api/v0/tokens/${t.id}/models`, { restrict, models: list });
			notice = 'Model allowlist saved.';
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function saveQuota(t: TokenSummary) {
		notice = null;
		try {
			await adminPost(`/api/v0/tokens/${t.id}/quota`, {
				dimension: quotaDimension,
				window: quotaWindow,
				value: parseFloat(quotaValue)
			});
			notice = 'Quota saved.';
		} catch (err) {
			notice = String(err);
		}
	}

	/** An `ask`-level MCP connector has nobody to prompt when the caller is a
	 * token, so it blocks by default; this is the owner's explicit opt-in. */
	async function saveMcpPolicy(t: TokenSummary, allow: boolean) {
		notice = null;
		try {
			await adminPut(`/api/v0/tokens/${t.id}/mcp-policy`, { allow });
			mcpAllow[t.id] = allow;
			notice = allow
				? 'Connectors that ask for approval will run for this token.'
				: 'Connectors that ask for approval are blocked for this token.';
		} catch (err) {
			notice = String(err);
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
						{#if expanded === token.id}
							<div class="mt-3 pl-1 flex flex-col gap-3">
								<div>
									<div class="text-xs font-medium mb-1">Model allowlist</div>
									<div class="flex flex-wrap gap-2 items-center">
										<label class="label cursor-pointer gap-1">
											<input type="checkbox" class="checkbox checkbox-xs" bind:checked={restrict} />
											<span class="label-text text-xs">Restrict to specific models</span>
										</label>
										{#if restrict}
											<input class="input input-bordered input-xs flex-1 min-w-48" placeholder="model ids, comma-separated" bind:value={modelsInput} />
										{/if}
										<button class="btn btn-ghost btn-xs" onclick={() => saveModels(token)}>Save models</button>
									</div>
								</div>
								<div>
									<div class="text-xs font-medium mb-1">Quota</div>
									<div class="flex flex-wrap gap-2 items-center">
										<select class="select select-bordered select-xs" bind:value={quotaDimension}>
											<option value="requests">Requests</option>
											<option value="tokens">Tokens</option>
											<option value="cost">Cost</option>
										</select>
										<span class="text-xs text-base-content/60">per</span>
										<select class="select select-bordered select-xs" bind:value={quotaWindow}>
											<option value="hour">Hour</option>
											<option value="day">Day</option>
											<option value="week">Week</option>
											<option value="month">Month</option>
										</select>
										<input class="input input-bordered input-xs w-24" type="number" min="0" placeholder="max" bind:value={quotaValue} />
										<button class="btn btn-ghost btn-xs" onclick={() => saveQuota(token)}>Add quota</button>
									</div>
								</div>
								<div>
									<div class="text-xs font-medium mb-1">MCP connectors</div>
									<div class="flex flex-wrap gap-2 items-center">
										<span class="text-xs text-base-content/60">
											Connectors that ask for approval have nobody to ask when the caller is a
											token.
										</span>
										<button
											class="btn btn-ghost btn-xs"
											onclick={() => saveMcpPolicy(token, !(mcpAllow[token.id] ?? false))}
										>
											{(mcpAllow[token.id] ?? false) ? 'Block them' : 'Let them run'}
										</button>
									</div>
								</div>
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
