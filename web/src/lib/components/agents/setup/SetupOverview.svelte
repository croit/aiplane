<script lang="ts">
	import Modal from '#lib/components/ui/Modal.svelte';
	import ArchitectModal from '#lib/components/agents/ArchitectModal.svelte';
	import StatusPill from '#lib/components/ui/StatusPill.svelte';
	import { ensureShape, agentUiCapabilities, type Spec } from '#lib/agents.js';
	import { visibleSetupSteps, checklist, sectionStatus, summary, type SectionStatus, type StepKey } from '#lib/agent-setup.js';
	import { emptyPlan, type GrantPlan } from '#lib/agent-grant-plan.js';
	import { useWorkspace } from '#lib/agent-workspace.svelte.js';
	import { t } from '#lib/i18n.svelte.js';
	import SetupChecklist from './SetupChecklist.svelte';
	import SetupStep from './SetupStep.svelte';
	import WidgetPreview from './WidgetPreview.svelte';

	/**
	 * The agent in plain language: one line per section with its status and an
	 * Edit that opens that section's step in a centred modal, the pre-publish
	 * checklist, and a sketch of the widget. The modal edits a copy of the
	 * buffer and stages its grants; Apply puts it back and saves both, Cancel
	 * drops them.
	 */
	let { onadvanced }: { onadvanced: () => void } = $props();
	const ws = useWorkspace();

	const ICONS: Partial<Record<StepKey, string>> = {
		basics: '🧑‍💼',
		scope: '🎯',
		abilities: '📚',
		slots: '📝',
		identity: '🔐',
		routes: '🔀',
		site: '🌐'
	};
	const PILL: Record<SectionStatus, { tone: 'ok' | 'warn' | 'neutral'; key: string }> = {
		done: { tone: 'ok', key: 'agents-setup-status-done' },
		open: { tone: 'warn', key: 'agents-setup-status-open' },
		optional: { tone: 'neutral', key: 'agents-setup-status-optional' }
	};
	const SIZE: Partial<Record<StepKey, 'md' | 'lg'>> = { scope: 'md', slots: 'lg' };

	const capabilities = $derived(agentUiCapabilities(ws.detail));
	const sections = $derived(visibleSetupSteps(ws.detail).filter((step) => step !== 'start' && step !== 'review'));
	const todos = $derived(checklist(ws.spec, ws.dirty ? [] : (ws.detail?.publish_issues ?? []), ws.resources?.defaults, ws.detail));
	const ctx = $derived({ tr: t, grants: ws.grants, resources: ws.resources, agents: ws.agents });

	let editing = $state<StepKey | null>(null);
	let open = $state(false);
	let draft = $state<Spec>({});
	let before = '';
	let planBefore: GrantPlan = emptyPlan();
	let applied = false;
	let applying = $state(false);
	let failed = $state(false);
	let planning = $state(false);

	/** The architect changed the draft: show it, keeping any unsaved edit of the person's. */
	function architectChanged() {
		void ws.refresh(ws.dirty);
	}

	function edit(step: StepKey | null) {
		if (!step) return onadvanced();
		before = JSON.stringify(ws.spec);
		planBefore = $state.snapshot(ws.plan);
		applied = false;
		draft = ensureShape(JSON.parse(before));
		editing = step;
		failed = false;
		open = true;
	}

	async function apply() {
		applying = true;
		ws.replace(draft);
		const saved = await ws.save();
		applying = false;
		applied = saved;
		if (saved) open = false;
		else failed = true;
	}

	/** Cancel (or a refused Apply) leaves the buffer and the staged grants as they were before the edit. */
	function closed() {
		if (!applied) ws.plan = planBefore;
		if (failed) ws.replace(JSON.parse(before));
		failed = false;
		editing = null;
	}
</script>

<div class="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_20rem]">
	<div class="flex min-w-0 flex-col gap-3">
		{#if ws.writable}
			<div class="flex flex-wrap items-center gap-3 rounded-box border border-primary/40 bg-primary/10 p-4">
				<div class="min-w-56 flex-1">
					<h2 class="m-0 text-base font-semibold">{t('agents-setup-cta-title')}</h2>
					<p class="m-0 text-sm text-base-content/70">{t('agents-setup-cta-text')}</p>
				</div>
				<div class="flex flex-wrap gap-2">
					<button class="btn" type="button" onclick={() => (planning = true)}>🎙 {t('architect-open')}</button>
					<a class="btn btn-primary" href="/agents/{ws.id}/setup/start">{t('agents-setup-cta-start')}</a>
				</div>
			</div>
		{/if}

		<ul class="m-0 flex list-none flex-col gap-2.5 p-0">
			{#each sections as step (step)}
				{@const status = sectionStatus(step, ws.spec, todos)}
				<li class="grid grid-cols-[2.5rem_minmax(0,1fr)_auto] items-center gap-3.5 rounded-box border bg-base-200 p-3.5 {status === 'open' ? 'border-warning/50' : 'border-base-300'}">
					<span class="grid size-10 place-items-center rounded-field bg-base-300 text-lg" aria-hidden="true">{ICONS[step]}</span>
					<div class="min-w-0">
						<h3 class="m-0 flex flex-wrap items-center gap-2 text-base font-semibold">
							{t(`agents-setup-step-${step}`)}
							<StatusPill size="sm" tone={PILL[status].tone}>{t(PILL[status].key)}</StatusPill>
						</h3>
						<p class="m-0 mt-0.5 break-words text-sm text-base-content/60">{summary(step, ws.spec, ctx)}</p>
					</div>
					{#if ws.writable}
						<button class="btn btn-sm" type="button" onclick={() => edit(step)}>{t('agents-setup-edit')}</button>
					{/if}
				</li>
			{/each}
		</ul>

		<p class="m-0 text-sm text-base-content/60">
			{t('agents-setup-advanced-hint')}
			<button class="link link-primary" type="button" onclick={onadvanced}>{t('agents-setup-advanced')}</button>
		</p>
	</div>

	<aside class="flex flex-col gap-3.5">
		<section class="card flex flex-col gap-3 p-4">
			<h3 class="m-0 text-base font-semibold">{t('agents-setup-checklist')}</h3>
			<SetupChecklist {todos} spec={ws.spec} onfix={edit} />
		</section>
		{#if capabilities.site}<section class="card flex flex-col gap-3 p-4">
			<h3 class="m-0 text-base font-semibold">{t('agents-setup-preview')}</h3>
			<WidgetPreview spec={ws.spec} />
		</section>{/if}
	</aside>
</div>

{#snippet actions()}
	<span class="mr-auto text-sm text-base-content/60">{t('agents-setup-modal-note')}</span>
	<button class="btn btn-ghost" type="button" onclick={() => (open = false)}>{t('admin-cancel')}</button>
	<button class="btn btn-primary" type="button" disabled={applying} onclick={() => void apply()}>{t('agents-setup-apply')}</button>
{/snippet}

<ArchitectModal bind:open={planning} agentId={ws.id} agentName={ws.spec.profile?.display || ws.detail?.display || null} onchanged={architectChanged} />

{#if editing}
	<Modal bind:open title={t(`agents-setup-step-${editing}`)} size={SIZE[editing] ?? 'lg'} footer={actions} onclose={closed}>
		<div class="flex flex-col gap-4 pb-1">
			{#if failed && ws.error}
				<div class="alert alert-error text-sm" role="alert">
					<div>
						<p class="m-0">{ws.error}</p>
						{#if ws.shownIssues.length > 1}
							<ul class="m-0 mt-1 list-inside list-disc">
								{#each ws.shownIssues as issue (issue.path + issue.message)}<li>{issue.message}</li>{/each}
							</ul>
						{/if}
					</div>
				</div>
			{/if}
			<SetupStep step={editing} bind:spec={draft} onfix={edit} />
		</div>
	</Modal>
{/if}
