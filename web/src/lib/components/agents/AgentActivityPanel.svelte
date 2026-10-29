<script lang="ts">
	import { agentActivityApi } from '#lib/owned-agents.js';
	import { summarize, type ActivityEvent } from '#lib/agent-activity.js';
	import { dt, t } from '#lib/i18n.svelte.js';
	import type { OwnedAgent } from '#lib/agent-state.js';

	let { agent, tabbed = false, active = false }: { agent: OwnedAgent; tabbed?: boolean; active?: boolean } = $props();
	let events = $state<ActivityEvent[]>([]);
	let next = $state<number | null>(null);
	let loading = $state(false);
	let loaded = $state(false);
	let error = $state<string | null>(null);
	let requested = 0;

	async function load(more = false) {
		const mine = ++requested;
		loading = true;
		try {
			const page = await agentActivityApi.list(agent.id, more ? next : null);
			if (mine !== requested) return;
			events = more ? [...events, ...page.events] : page.events;
			next = page.next_cursor;
			loaded = true;
			error = null;
		} catch (caught) {
			if (mine === requested) {
				error = caught instanceof Error ? caught.message : t('agent-activity-error-load');
			}
		} finally {
			if (mine === requested) loading = false;
		}
	}

	function onToggle(event: Event) {
		const details = event.currentTarget;
		if (details instanceof HTMLDetailsElement && details.open && !loaded && !loading) void load();
	}

	$effect(() => {
		if (tabbed && active && !loaded && !loading && !error) void load();
	});

	const line = (event: ActivityEvent) => {
		const summary = summarize(event);
		return summary ? t(summary.key, summary.args) : event.kind;
	};
</script>

{#snippet panelBody()}
	<div class="space-y-3 text-sm">
		{#if error}<div class="alert alert-error py-2" role="alert"><span>{error}</span><button class="btn btn-ghost btn-xs" type="button" onclick={() => void load()}>{t('agent-activity-retry')}</button></div>{/if}
		{#if loading && !events.length}
			<div class="skeleton h-10 w-full"></div>
		{:else if loaded && !events.length}
			<p class="text-base-content/60">{t('agent-activity-empty')}</p>
		{:else}
			<ul class="divide-y divide-base-300">
				{#each events as event (event.id)}
					<li class="flex flex-col gap-1 py-2">
						<div class="flex flex-wrap items-start gap-2">
							<span class="badge badge-outline badge-sm">{event.kind}</span>
							<time class="text-xs text-base-content/60">{dt(event.ts)}</time>
						</div>
						<p class="mb-0 text-sm">{line(event)}</p>
						<details class="text-xs">
							<summary class="cursor-pointer text-base-content/60">{t('agent-activity-details')}</summary>
							<pre class="mt-2 max-h-48 overflow-auto rounded-box bg-base-200 p-2 whitespace-pre-wrap break-words">{JSON.stringify(event.detail, null, 2)}</pre>
						</details>
					</li>
				{/each}
			</ul>
		{/if}
		{#if next !== null}<button class="btn btn-sm" type="button" disabled={loading} onclick={() => void load(true)}>{loading ? t('agent-activity-loading-more') : t('agent-activity-more')}</button>{/if}
	</div>
{/snippet}

{#if tabbed}
	<section hidden={!active} class="min-w-0" data-agent-panel="activity">{@render panelBody()}</section>
{:else}
	<details class="collapse collapse-arrow border border-base-300 bg-base-100" ontoggle={onToggle}>
		<summary class="collapse-title min-h-0 py-3 text-sm font-medium">{t('agent-activity-heading')}</summary>
		<div class="collapse-content">{@render panelBody()}</div>
	</details>
{/if}
