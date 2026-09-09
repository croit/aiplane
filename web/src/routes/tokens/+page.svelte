<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { TokenSummary } from '$lib/api';
	import { adminPut, adminPost } from '$lib/admin-client';
	import { t, dt } from '$lib/i18n.svelte';

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
	let restrict = $state(false);
	let quotaDimension = $state('requests');
	let quotaWindow = $state('day');
	let quotaValue = $state('');
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

	function togglePanel(token: TokenSummary) {
		if (expanded === token.id) {
			expanded = null;
			return;
		}
		expanded = token.id;
		quotaValue = '';
	}

	async function saveModels(token: TokenSummary) {
		notice = null;
		try {
			const list = modelsInput
				.split(',')
				.map((m) => m.trim())
				.filter(Boolean);
			await adminPut(`/api/v0/tokens/${token.id}/models`, { restrict, models: list });
			notice = restrict
				? t('tokens-models-saved-toast', { count: list.length })
				: t('tokens-models-cleared-toast');
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function saveQuota(token: TokenSummary) {
		notice = null;
		try {
			await adminPost(`/api/v0/tokens/${token.id}/quota`, {
				dimension: quotaDimension,
				window: quotaWindow,
				value: parseFloat(quotaValue)
			});
			notice = t('tokens-limits-saved-toast');
		} catch (err) {
			notice = String(err);
		}
	}

	/** An `ask`-level MCP connector has nobody to prompt when the caller is a
	 * token, so it blocks by default; this is the owner's explicit opt-in. */
	async function saveMcpPolicy(token: TokenSummary, allow: boolean) {
		notice = null;
		try {
			await adminPut(`/api/v0/tokens/${token.id}/mcp-policy`, { allow });
			mcpAllow[token.id] = allow;
			notice = allow
				? t('tokens-mcp-ask-enabled-toast')
				: t('tokens-mcp-ask-disabled-toast');
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
		if (!confirm(t('tokens-revoke-confirm'))) return;
		try {
			await api.revokeToken(id);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function rotate(id: string) {
		if (!confirm(t('tokens-rotate-confirm'))) return;
		try {
			const res = await api.rotateToken(id);
			minted = { name: res.token.name, plaintext: res.plaintext };
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function remove(id: string) {
		if (!confirm(t('tokens-remove-confirm'))) return;
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
	<h1 class="text-2xl font-bold">{t('tokens-page-heading')}</h1>
</div>

<p class="text-base-content/60 text-sm mb-6">
	{t('tokens-intro')}
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
				<span class="text-success">✓</span>
				{t('tokens-minted-heading')}
			</h2>
			<p class="text-sm text-base-content/70">{t('tokens-minted-copy-warning')}</p>
			<div class="relative">
				<pre class="bg-base-100 border border-base-300 rounded-md p-3 pr-12 m-0 font-mono text-xs select-all break-all whitespace-pre-wrap">{minted.plaintext}</pre>
				<button
					class="btn btn-ghost btn-sm btn-square absolute top-1.5 right-1.5"
					onclick={copyPlaintext}
					aria-label={t('tokens-copy-aria')}
					title={t('tokens-copy-title')}
				>
					{t('webhooks-copy')}
				</button>
			</div>
			<p class="text-xs text-base-content/60 mt-2">
				{t('tokens-minted-name', { name: minted.name })}
			</p>
		</div>
	</div>
{/if}

<div class="card border border-base-300 mb-6">
	<div class="card-body">
		<h2 class="card-title">{t('tokens-create-heading')}</h2>
		<div class="flex flex-wrap items-end gap-3">
			<label class="flex flex-col gap-1 flex-1 min-w-48">
				<span class="label-text">{t('tokens-name-label')}</span>
				<input
					class="input input-bordered w-full"
					placeholder={t('tokens-name-placeholder')}
					bind:value={name}
					onkeydown={(e) => e.key === 'Enter' && create()}
				/>
			</label>
			<label class="flex flex-col gap-1 w-32">
				<span class="label-text">{t('tokens-ttl-label')}</span>
				<input class="input input-bordered w-full" type="number" min="1" max="1825" bind:value={ttlDays} />
			</label>
			<button class="btn btn-primary" onclick={create} disabled={!name.trim() || busy}>
				{t('tokens-create-submit')}
			</button>
		</div>
	</div>
</div>

<div class="card border border-base-300">
	<div class="card-body">
		<h2 class="card-title">{t('tokens-list-heading')}</h2>
		{#if tokens.length === 0}
			<p class="text-base-content/60 text-sm">{t('tokens-list-empty')}</p>
		{:else}
			<ul class="flex flex-col divide-y divide-base-300">
				{#each tokens as token (token.id)}
					<li class="py-3">
						<div class="flex items-center gap-4">
							<div class="flex-1 min-w-0">
								<button class="text-sm font-medium link link-hover" onclick={() => togglePanel(token)}>
									{token.name}
								</button>
								<div class="text-xs text-base-content/60">
									{t('tokens-row-meta', {
										created: dt(token.created_at, { dateStyle: 'medium' }),
										last_used: token.last_used_at
											? dt(token.last_used_at, { dateStyle: 'medium' })
											: t('tokens-last-used-never'),
										expires: dt(token.expires_at, { dateStyle: 'medium' })
									})}
								</div>
							</div>
							{#if token.revoked}
								<span class="badge badge-error">{t('tokens-badge-revoked')}</span>
							{:else}
								<span class="badge badge-secondary">{t('tokens-badge-active')}</span>
							{/if}
							<label class="flex items-center gap-2 text-sm">
								<input
									type="checkbox"
									class="toggle toggle-primary toggle-sm"
									checked={token.tools_enabled}
									disabled={token.revoked}
									onchange={() => toggleTools(token)}
									aria-label={t('tokens-tool-use-aria')}
								/>
								<span class="text-base-content/60">{t('tokens-tool-use-label')}</span>
							</label>
							{#if !token.revoked}
								<button
									class="btn btn-outline btn-sm"
									onclick={() => rotate(token.id)}
									title={t('tokens-rotate-title')}
								>
									{t('tokens-rotate-button')}
								</button>
								<button class="btn btn-error btn-sm" onclick={() => revoke(token.id)}>{t('tokens-revoke-button')}</button>
							{:else}
								<button class="btn btn-outline btn-sm" onclick={() => remove(token.id)}>{t('tokens-remove-button')}</button>
							{/if}
						</div>
						{#if expanded === token.id}
							<div class="mt-3 pl-1 flex flex-col gap-3">
								<div>
									<div class="text-xs font-medium mb-1">{t('tokens-models-heading')}</div>
									<div class="flex flex-wrap gap-2 items-center">
										<label class="label cursor-pointer gap-1">
											<input type="checkbox" class="checkbox checkbox-xs" bind:checked={restrict} />
											<span class="label-text text-xs">{t('tokens-models-restrict-label')}</span>
										</label>
										{#if restrict}
											<input
												class="input input-bordered input-xs flex-1 min-w-48"
												placeholder={t('tokens-models-input-placeholder')}
												bind:value={modelsInput}
											/>
										{/if}
										<button class="btn btn-ghost btn-xs" onclick={() => saveModels(token)}>{t('tokens-models-save')}</button>
									</div>
								</div>
								<div>
									<div class="text-xs font-medium mb-1">{t('tokens-quota-heading')}</div>
									<div class="flex flex-wrap gap-2 items-center">
										<select class="select select-bordered select-xs" bind:value={quotaDimension}>
											<option value="requests">{t('limits-dim-requests')}</option>
											<option value="tokens">{t('limits-dim-tokens')}</option>
											<option value="cost">{t('limits-dim-cost-short')}</option>
										</select>
										<span class="text-xs text-base-content/60">{t('tokens-quota-per')}</span>
										<select class="select select-bordered select-xs" bind:value={quotaWindow}>
											<option value="hour">{t('limits-win-hour')}</option>
											<option value="day">{t('limits-win-day')}</option>
											<option value="week">{t('limits-win-week')}</option>
											<option value="month">{t('limits-win-month')}</option>
										</select>
										<input
											class="input input-bordered input-xs w-24"
											type="number"
											min="0"
											placeholder={t('tokens-quota-max-placeholder')}
											bind:value={quotaValue}
										/>
										<button class="btn btn-ghost btn-xs" onclick={() => saveQuota(token)}>{t('tokens-limits-add')}</button>
									</div>
								</div>
								<div>
									<div class="text-xs font-medium mb-1">{t('tokens-mcp-heading')}</div>
									<div class="flex flex-wrap gap-2 items-center">
										<span class="text-xs text-base-content/60">
											{t('tokens-mcp-allow-description')}
										</span>
										<button
											class="btn btn-ghost btn-xs"
											onclick={() => saveMcpPolicy(token, !(mcpAllow[token.id] ?? false))}
										>
											{(mcpAllow[token.id] ?? false)
												? t('tokens-mcp-block-button')
												: t('tokens-mcp-allow-button')}
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
