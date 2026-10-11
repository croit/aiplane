<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { agentUiCapabilities, testChatSources, type Spec } from '#lib/agents.js';
	import { useWorkspace } from '#lib/agent-workspace.svelte.js';
	import { t } from '#lib/i18n.svelte.js';
	import AgentEditor from './AgentEditor.svelte';
	import AgentCanvas from './AgentCanvas.svelte';
	import ActivityPanel from './ActivityPanel.svelte';
	import AnalyticsPanel from './AnalyticsPanel.svelte';
	import TestsPanel from './TestsPanel.svelte';
	import GrantsPanel from './GrantsPanel.svelte';
	import SharingPanel from './SharingPanel.svelte';
	import ChannelsPanel from './ChannelsPanel.svelte';
	import EmbedKeysPanel from './EmbedKeysPanel.svelte';
	import SpecJsonEditor from './SpecJsonEditor.svelte';
	import TestChat from './TestChat.svelte';
	import VersionsPanel from './VersionsPanel.svelte';
	import SetupOverview from './setup/SetupOverview.svelte';
	import DotConnectionsPanel from '../dots/DotConnectionsPanel.svelte';
	import DotMemoryPanel from '../dots/DotMemoryPanel.svelte';
	import type { Dot } from '#lib/dots.js';

	/**
	 * One agent's page, in four tabs: **Setup** (the plain-language overview,
	 * or behind a switch the advanced editor — form, canvas, JSON, grants —
	 * on the same buffer), **Try it** (test chat, test cases), **Insights**
	 * (analytics, activity) and **Settings** (versions, sharing including who
	 * answers the inbox, channels, embed keys). The choice lives in the URL:
	 * `?tab=`, `?view=advanced`, `?sub=`.
	 */
	const ws = useWorkspace();

	const capabilities = $derived(agentUiCapabilities(ws.detail));
	const organizationDot = $derived<Dot | null>(ws.detail?.dot?.owner.kind === 'organization' ? {
		id: ws.detail.id, name: ws.detail.dot.name, display: ws.detail.display,
		description: ws.detail.description, active: ws.detail.dot.active,
	} : null);
	const TABS = {
		setup: [] as string[],
		try: ['test', 'tests'],
		insights: ['analytics', 'activity'],
		settings: ['versions', 'sharing']
	} as const;
	type Tab = keyof typeof TABS;
	const tabs = $derived((Object.keys(TABS) as Tab[]).filter((name) => name !== 'try' || capabilities.draftChat || capabilities.execution));
	const ADVANCED = ['edit', 'canvas', 'json', 'grants'] as const;

	const param = (name: string) => page.url.searchParams.get(name);
	const tab = $derived(tabs.find((x) => x === param('tab')) ?? 'setup');
	const advanced = $derived(tab === 'setup' && param('view') === 'advanced');
	const subs = $derived<readonly string[]>(advanced ? ADVANCED : tab === 'try'
		? TABS.try.filter((name) => name === 'test' ? capabilities.draftChat : capabilities.execution)
		: TABS[tab]);
	const sub = $derived(subs.find((x) => x === param('sub')) ?? subs[0] ?? '');

	function navigate(next: Record<string, string | null>) {
		const url = new URL(page.url.href);
		for (const [key, value] of Object.entries(next)) {
			if (value === null) url.searchParams.delete(key);
			else url.searchParams.set(key, value);
		}
		void goto(url, { replace: true, reset: false });
	}

	function applyJson(parsed: Spec) {
		ws.replace(parsed);
	}
	const save = async () => void (await ws.save());
	const toGrants = () => navigate({ tab: 'setup', view: 'advanced', sub: 'grants' });
</script>

<div role="tablist" class="tabs tabs-border w-full overflow-x-auto border-b border-base-300">
	{#each tabs as name (name)}
		<button role="tab" type="button" class="tab whitespace-nowrap" class:tab-active={tab === name} aria-selected={tab === name} onclick={() => navigate({ tab: name, sub: null, view: null })}>
			{t(`agents-tab-${name}`)}
		</button>
	{/each}
</div>

{#if tab === 'setup'}
	<div class="flex flex-wrap items-center gap-3">
		<label class="flex cursor-pointer items-center gap-2 text-sm">
			<input type="checkbox" class="toggle toggle-sm" checked={advanced} onchange={(e) => navigate({ view: e.currentTarget.checked ? 'advanced' : null, sub: null })} />
			{t('agents-setup-advanced')}
		</label>
		{#if advanced}<span class="text-sm text-base-content/60">{t('agents-setup-advanced-hint')}</span>{/if}
	</div>
{/if}

{#if subs.length}
	<div role="tablist" class="tabs tabs-box tabs-sm w-fit max-w-full overflow-x-auto">
		{#each subs as name (name)}
			<button role="tab" type="button" class="tab whitespace-nowrap" class:tab-active={sub === name} aria-selected={sub === name} onclick={() => navigate({ sub: name })}>
				{t(`agents-tab-${name}`)}
			</button>
		{/each}
	</div>
{/if}

{#if ws.writable && organizationDot}<a class="btn btn-sm w-fit" href={`/scheduled?principal_id=${encodeURIComponent(ws.id)}`}>{t('scheduled-heading')}</a>{/if}
{#if ws.writable && organizationDot}<a class="btn btn-sm w-fit" href={`/webhooks?principal_id=${encodeURIComponent(ws.id)}`}>{t('webhooks-heading')}</a>{/if}

{#if tab === 'setup' && ws.writable && organizationDot}
	{#key organizationDot.id}
		<DotConnectionsPanel dot={organizationDot} onchange={() => void ws.refresh(true).catch((error) => ws.fail(error))} />
	{/key}
{/if}

{#if tab === 'setup' && organizationDot && (ws.detail?.access === 'read' || ws.detail?.access === 'write')}
	{#key organizationDot.id}
		<DotMemoryPanel dot={organizationDot} />
	{/key}
{/if}

<div data-agent-tab class={sub === 'test' ? 'flex min-h-0 flex-1 flex-col' : 'pt-1'}>
	{#if tab === 'setup' && !advanced}
		<SetupOverview onadvanced={() => navigate({ view: 'advanced', sub: null })} />
	{:else if sub === 'edit'}
		{#key ws.formKey}
			<fieldset disabled={!ws.writable} class="min-w-0">
				<AgentEditor bind:spec={ws.spec} issues={ws.shownIssues} granted={ws.granted} agents={ws.agents} ongrants={toGrants} {capabilities} />
			</fieldset>
		{/key}
	{:else if sub === 'canvas'}
		<fieldset disabled={!ws.writable} class="min-w-0">
			<AgentCanvas bind:spec={ws.spec} issues={ws.shownIssues} granted={ws.granted} agents={ws.agents} lastDebug={ws.lastDebug} ongrants={toGrants} />
		</fieldset>
	{:else if sub === 'json'}
		{#key ws.formKey}
			<SpecJsonEditor spec={ws.spec} issues={ws.shownIssues} onapply={applyJson} />
		{/key}
	{:else if sub === 'grants' && ws.detail}
		<GrantsPanel agentId={ws.id} grants={ws.detail.grants} resources={ws.resources} writable={ws.writable} onchanged={() => ws.refresh(true)} />
	{:else if sub === 'test' && capabilities.draftChat}
		<TestChat agentId={ws.id} spec={ws.spec} dirty={ws.dirty} organization={!!organizationDot} sources={testChatSources(ws.detail)} liveVersion={ws.detail?.live_version ?? null} initialSessionId={param('test_session')} onsession={(sessionId) => navigate({ test_session: sessionId })} onturn={(debug) => (ws.lastDebug = debug)} onsave={save} />
	{:else if sub === 'tests' && capabilities.execution && ws.detail}
		<TestsPanel agentId={ws.id} versions={ws.versions} liveVersion={ws.detail.live_version} dirty={ws.dirty} writable={ws.writable} onsave={save} />
	{:else if sub === 'analytics'}
		<AnalyticsPanel agentId={ws.id} versions={ws.versions} />
	{:else if sub === 'activity'}
		<ActivityPanel agentId={ws.id} />
	{:else if sub === 'versions' && ws.detail}
		<VersionsPanel
			agentId={ws.id}
			liveVersion={ws.detail.live_version}
			versions={ws.versions}
			publishIssues={ws.detail.publish_issues}
			dirty={ws.dirty}
			writable={ws.writable}
			onpublish={async () => void (await ws.publish())}
			onlive={(version) => ws.madeLive(version)}
		/>
	{:else if sub === 'sharing' && ws.detail}
		<div class="space-y-6">
			<SharingPanel agentId={ws.id} shares={ws.detail.shares} writable={ws.writable} onchanged={() => ws.refresh(true)} />
			{#if capabilities.channels}<ChannelsPanel agentId={ws.id} writable={ws.writable} />{/if}
			{#if capabilities.embed}<EmbedKeysPanel agentId={ws.id} writable={ws.writable} />{/if}
		</div>
	{/if}
</div>
