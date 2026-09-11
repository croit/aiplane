<script lang="ts">
	import { base } from '$app/paths';
	import type { ChatCapability } from '$lib/api';
	import { t } from '$lib/i18n.svelte';

	let { capabilities, onset }: {
		capabilities: ChatCapability[];
		onset: (capability: ChatCapability, state: ChatCapability['state']) => Promise<void>;
	} = $props();

	let open = $state(false);
	let query = $state('');
	let busy = $state(false);
	const active = $derived(capabilities.filter((capability) => capability.state === 'on'));
	const groups = $derived(
		Array.from(new Set(capabilities.map((capability) => capability.group))).map((name) => ({
			name,
			rows: capabilities.filter((capability) => capability.group === name)
		}))
	);

	function groupLabel(group: string): string {
		const key: Record<string, string> = {
			'Web & Network': 'chat-render-group-web-network',
			'Attachments & Documents': 'chat-render-group-attachments-documents',
			'Document templates': 'chat-render-group-document-templates',
			'Knowledge base': 'chat-render-group-knowledge-base',
			'Code & Sandbox': 'chat-render-group-code-sandbox',
			Memory: 'chat-render-group-memory',
			Integrations: 'chat-render-group-integrations',
			Utility: 'chat-render-group-utility',
			Skills: 'chat-render-group-skills'
		};
		return key[group] ? t(key[group]) : group;
	}

	function visible(capability: ChatCapability): boolean {
		const needle = query.trim().toLocaleLowerCase();
		return !needle || `${capability.title} ${capability.description}`.toLocaleLowerCase().includes(needle);
	}

	async function setMany(rows: ChatCapability[], state: ChatCapability['state']) {
		busy = true;
		try {
			await Promise.all(rows.map((row) => onset(row, state === 'off' && !row.can_disable ? 'auto' : state)));
		} finally {
			busy = false;
		}
	}

	function aggregate(rows: ChatCapability[]): ChatCapability['state'] | null {
		const first = rows[0]?.state;
		return first && rows.every((row) => row.state === first) ? first : null;
	}
</script>

{#snippet segmented(rows: ChatCapability[])}
	{@const selected = aggregate(rows)}
	<div class="join shrink-0">
		{#if rows.some((row) => row.can_disable)}
			<button type="button" class="btn btn-xs join-item {selected === 'off' ? 'btn-neutral' : 'btn-ghost'}" title={t('chat-render-state-off-tip')} aria-label={t('chat-render-state-off-tip')} disabled={busy} onclick={() => setMany(rows, 'off')}>×</button>
		{/if}
		<button type="button" class="btn btn-xs join-item {selected === 'auto' ? 'btn-neutral' : 'btn-ghost'}" title={t('chat-render-state-auto-tip')} aria-label={t('chat-render-state-auto-tip')} disabled={busy} onclick={() => setMany(rows, 'auto')}>A</button>
		<button type="button" class="btn btn-xs join-item {selected === 'on' ? 'btn-primary' : 'btn-ghost'}" title={t('chat-render-state-on-tip')} aria-label={t('chat-render-state-on-tip')} disabled={busy} onclick={() => setMany(rows, 'on')}>✓</button>
	</div>
{/snippet}

<div class="relative flex flex-wrap items-center gap-1.5">
	<button type="button" class="btn btn-ghost btn-sm gap-1 rounded-full" title={t('chat-render-tools-tooltip')} onclick={() => (open = !open)} aria-expanded={open}>
		<span aria-hidden="true">+</span> {t('chat-render-tools-label')}
	</button>
	{#if active.length > 0}
		<button type="button" class="badge badge-outline gap-1 sm:hidden" title={t('chat-render-active-count-title')} onclick={() => (open = true)}>⌁ {active.length}</button>
	{/if}
	{#each active as capability (`${capability.kind}:${capability.key}`)}
		<button type="button" class="badge badge-outline hidden gap-1 sm:inline-flex" title={t('chat-render-unpin-title')} onclick={() => onset(capability, 'auto')}>
			{capability.title} <span class="opacity-60">×</span>
		</button>
	{/each}

	{#if open}
		<button class="fixed inset-0 z-30 bg-black/35 sm:hidden" aria-label={t('chat-render-close')} onclick={() => (open = false)}></button>
		<div class="fixed inset-x-3 bottom-3 z-40 max-h-[80dvh] overflow-hidden rounded-box border border-base-300 bg-base-100 shadow-xl sm:absolute sm:inset-x-auto sm:bottom-full sm:left-0 sm:mb-2 sm:w-[30rem]">
			<div class="flex items-center gap-2 border-b border-base-300 p-2">
				<strong class="flex-1 text-sm">{t('chat-render-tools-label')}</strong>
				<button type="button" class="btn btn-ghost btn-xs btn-circle" aria-label={t('chat-render-close')} onclick={() => (open = false)}>×</button>
			</div>
			{#if capabilities.length > 0}
				<div class="p-2 pb-1">
					<input class="input input-bordered input-sm w-full" bind:value={query} placeholder={t('chat-render-tools-search-placeholder')} />
				</div>
				<div class="max-h-[60dvh] overflow-y-auto p-2 pt-1">
					<div class="sticky top-0 z-10 flex items-center gap-2 border-b border-base-300 bg-base-100 px-2 py-2">
						<strong class="flex-1 text-xs uppercase tracking-wide opacity-70">{t('chat-render-all-tools-label')}</strong>
						{@render segmented(capabilities)}
					</div>
					{#each groups as group (group.name)}
						{@const rows = group.rows.filter(visible)}
						{#if rows.length > 0}
							<div class="grid grid-cols-[1fr_auto] items-center border-b border-base-200">
								<details class="contents" open={query.trim().length > 0}>
									<summary class="col-start-1 row-start-1 cursor-pointer px-2 py-2 text-xs font-semibold uppercase tracking-wide">
										{groupLabel(group.name)} · {t('chat-render-tool-count', { count: rows.length })}
									</summary>
									<div class="col-span-2 px-1 pb-2">
									{#each rows as capability (`${capability.kind}:${capability.key}`)}
										<div class="rounded-lg px-2 py-2 hover:bg-base-200">
											<div class="flex items-center gap-2">
												<span class="min-w-0 flex-1 text-sm font-medium">{capability.title}</span>
												{@render segmented([capability])}
											</div>
											{#if capability.description}<p class="mt-1 text-xs text-base-content/60">{capability.description}</p>{/if}
										</div>
									{/each}
									</div>
								</details>
								<div class="col-start-2 row-start-1 pr-2">{@render segmented(group.rows)}</div>
							</div>
						{/if}
					{/each}
				</div>
			{:else}
				<p class="p-4 text-sm">{t('chat-render-no-tools-prefix')} <a class="link" href="{base}/integrations">{t('nav-integrations')}</a>{t('chat-render-no-tools-suffix')}</p>
			{/if}
		</div>
	{/if}
</div>
