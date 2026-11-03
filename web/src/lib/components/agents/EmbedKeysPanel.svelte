<script lang="ts">
	import { onMount } from 'svelte';
	import { agentsApi, embedSnippet, splitList, type AgentError, type EmbedKey } from '#lib/agents.js';
	import { locale, t } from '#lib/i18n.svelte.js';

	/**
	 * The keys websites embed this agent with (`docs/embed.md`). The server
	 * hands a new key out once, so its snippet is shown right after creation
	 * and never again; the list carries names and origins only.
	 */
	let { agentId, writable }: { agentId: string; writable: boolean } = $props();

	let keys = $state<EmbedKey[]>([]);
	let name = $state('');
	let origins = $state('');
	let snippet = $state<string | null>(null);
	let error = $state<string | null>(null);
	let busy = $state(false);

	async function run(action: () => Promise<unknown>) {
		busy = true;
		error = null;
		try {
			await action();
			keys = await agentsApi.embedKeys(agentId);
		} catch (err) {
			error = (err as AgentError).message;
		} finally {
			busy = false;
		}
	}

	onMount(() => void run(async () => {}));

	const create = () =>
		run(async () => {
			const created = await agentsApi.createEmbedKey(agentId, { name: name.trim(), origins: splitList(origins) });
			snippet = embedSnippet(`${window.location.origin}/embed.js`, created.key);
			name = '';
			origins = '';
		});
</script>

<section class="card card-border">
	<div class="card-body gap-4 p-4">
		<h2 class="card-title text-base">{t('agents-embed-heading')}</h2>
		<p class="text-sm text-base-content/70">{t('agents-embed-intro')}</p>
		{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}
		{#if snippet}
			<div class="alert alert-success flex-col items-stretch text-sm" role="status">
				<span>{t('agents-embed-created')}</span>
				<pre class="overflow-x-auto rounded-field bg-base-100 p-2 font-mono text-xs text-base-content select-all">{snippet}</pre>
			</div>
		{/if}

		<ul class="flex flex-col divide-y divide-base-300">
			{#each keys as key (key.id)}
				<li class="flex flex-wrap items-center gap-2 py-2">
					<span class="font-semibold">{key.name}</span>
					{#each key.origins as origin (origin)}<span class="badge badge-outline badge-sm font-mono">{origin}</span>{/each}
					<span class="text-xs text-base-content/60">
						{t('agents-embed-created-by', { user: key.created_by, at: new Date(key.created_at).toLocaleDateString(locale.current) })}
					</span>
					{#if key.revoked_at}
						<span class="badge badge-ghost badge-sm ml-auto">{t('agents-embed-revoked')}</span>
					{:else if writable}
						<button class="btn btn-ghost btn-xs ml-auto" type="button" disabled={busy} onclick={() => run(() => agentsApi.revokeEmbedKey(agentId, key.id))}>{t('agents-embed-revoke')}</button>
					{/if}
				</li>
			{:else}
				<li class="py-2 text-sm text-base-content/60">{t('agents-embed-empty')}</li>
			{/each}
		</ul>

		{#if writable}
			<form class="flex flex-col gap-3" onsubmit={(e) => { e.preventDefault(); void create(); }}>
				<label class="flex flex-col gap-1">
					<span class="label-text">{t('agents-embed-name')}</span>
					<input class="input w-64 max-w-full" bind:value={name} placeholder={t('agents-embed-name-hint')} required />
				</label>
				<label class="flex flex-col gap-1">
					<span class="label-text">{t('agents-embed-origins')}</span>
					<textarea class="textarea w-full max-w-lg font-mono" rows="2" bind:value={origins} placeholder="https://www.example.com" required></textarea>
					<span class="text-xs text-base-content/60">{t('agents-embed-origins-help')}</span>
				</label>
				<div><button class="btn btn-primary" type="submit" disabled={busy || !name.trim() || !origins.trim()}>{t('agents-embed-create')}</button></div>
			</form>
		{/if}
	</div>
</section>
