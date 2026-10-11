<script lang="ts">
	import AiSuggestion from '#lib/components/ui/AiSuggestion.svelte';
	import { agentsApi, agentUiCapabilities, type AgentError, type Spec } from '#lib/agents.js';
	import { visibleSetupSteps, checklist, setupErrorMessage, summary, type StepKey } from '#lib/agent-setup.js';
	import { useWorkspace } from '#lib/agent-workspace.svelte.js';
	import { t } from '#lib/i18n.svelte.js';
	import SetupChecklist from './SetupChecklist.svelte';

	/** The last step: the whole setup in plain sentences, what is still open, and the way to the test chat. */
	let { spec = $bindable(), onfix }: { spec: Spec; onfix: (step: StepKey | null) => void } = $props();
	const ws = useWorkspace();

	const capabilities = $derived(agentUiCapabilities(ws.detail));
	const sections = $derived(visibleSetupSteps(ws.detail).filter((step) => step !== 'start' && step !== 'review'));
	const ctx = $derived({ tr: t, grants: ws.grants, resources: ws.resources, agents: ws.agents });
	const todos = $derived(checklist(spec, ws.dirty ? [] : (ws.detail?.publish_issues ?? []), ws.resources?.defaults, ws.detail));
	const name = $derived(spec.profile?.display || ws.detail?.display || ws.detail?.name || '');

	const tests = $derived(ws.suggestion?.steps.tests ?? []);
	let saved = $state<string[]>([]);
	let testError = $state<string | null>(null);
	const unsaved = $derived(tests.filter((x) => !saved.includes(x.name)));
	let savingAll = $state(false);
	async function saveTest(test: (typeof tests)[number]): Promise<boolean> {
		testError = null;
		try {
			await agentsApi.createTest(ws.id, { name: test.name, script: test.script, expect: test.expect, rubric: null });
			saved = [...saved, test.name];
			return true;
		} catch (err) {
			testError = setupErrorMessage(err as AgentError, t);
			return false;
		}
	}
	async function saveAll() {
		savingAll = true;
		for (const test of unsaved) if (!(await saveTest(test))) break;
		savingAll = false;
	}
	const sayings = (test: (typeof tests)[number]) =>
		test.script.flatMap((s) => ('say' in s ? [s.say] : [])).join(' · ');
</script>

<div class="flex flex-col gap-5">
	<p class="m-0 text-base-content/70">{t('agents-setup-review-lead', { name })}</p>
	<dl class="card m-0 grid gap-x-4 gap-y-2 p-4 text-sm sm:grid-cols-[auto_minmax(0,1fr)]">
		{#each sections as step (step)}
			<dt class="font-semibold">{t(`agents-setup-step-${step}`)}</dt>
			<dd class="m-0 break-words text-base-content/80">{summary(step, spec, ctx)}</dd>
		{/each}
	</dl>
	<section class="card flex flex-col gap-3 p-4">
		<h3 class="m-0 text-base font-semibold">{t('agents-setup-checklist')}</h3>
		<SetupChecklist {todos} {spec} {onfix} />
	</section>
	{#if capabilities.execution && tests.length}
		<AiSuggestion label={t('agents-setup-suggest-tests')}>
			{#if unsaved.length > 1}
				<button class="btn btn-sm btn-primary mb-2" type="button" disabled={!ws.writable || savingAll} onclick={() => void saveAll()}>
					{#if savingAll}<span class="loading loading-spinner loading-xs"></span>{/if}
					{t('agents-setup-test-save-all')}
				</button>
			{/if}
			<ul class="m-0 flex list-none flex-col gap-2 p-0">
				{#each tests as test (test.name)}
					<li class="flex flex-wrap items-center gap-2">
						<span class="badge badge-outline">{t(test.kind === 'out_of_scope' ? 'agents-setup-test-kind-out' : 'agents-setup-test-kind-in')}</span>
						<span class="font-semibold">{test.name}</span>
						<span class="min-w-0 flex-1 break-words text-base-content/70">{sayings(test)}</span>
						{#if saved.includes(test.name)}
							<span class="text-success">✓ {t('agents-setup-test-saved')}</span>
						{:else}
							<button class="btn btn-sm" type="button" disabled={!ws.writable || savingAll} onclick={() => void saveTest(test)}>{t('agents-setup-test-save')}</button>
						{/if}
					</li>
				{/each}
			</ul>
			{#if testError}<p class="m-0 mt-2 text-error" role="alert">{testError}</p>{/if}
		</AiSuggestion>
	{/if}
	{#if capabilities.execution}<a class="link link-primary text-sm" href="/agents/{ws.id}?tab=try">{t('agents-setup-review-try')}</a>{/if}
</div>
