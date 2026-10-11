<script lang="ts">
	import StepIndicator from '#lib/components/ui/StepIndicator.svelte';
	import { goto } from '$app/navigation';
	import { visibleSetupSteps, asStep, type StepKey } from '#lib/agent-setup.js';
	import { useWorkspace } from '#lib/agent-workspace.svelte.js';
	import { t } from '#lib/i18n.svelte.js';
	import SetupStep from './SetupStep.svelte';

	/**
	 * The setup assistant as its own page, `/agents/[id]/setup/[step]`: every
	 * step is a URL, so a step can be linked, the browser's back button goes
	 * one step back, and a reload lands where it was. Moving on saves the
	 * draft first, which is what makes the reload safe; a refused save keeps
	 * the person on the step, with the server's reasons in the header.
	 * The footer keeps clear of the feedback button in the bottom corner.
	 */
	let { step: param }: { step: string } = $props();
	const ws = useWorkspace();

	const steps = $derived(visibleSetupSteps(ws.detail));
	const requestedStep = $derived(asStep(param));
	const step = $derived(requestedStep && steps.includes(requestedStep) ? requestedStep : null);
	const index = $derived(step ? steps.indexOf(step) : 0);
	const labels = $derived(steps.map((s) => t(`agents-setup-step-${s}-short`)));
	const completed = $derived(steps.map((_, i) => i).filter((i) => i < index));
	const name = $derived(ws.spec.profile?.display || ws.detail?.display || ws.detail?.name || '');

	const href = (to: StepKey) => `/agents/${ws.id}/setup/${to}`;
	const overview = () => `/agents/${ws.id}?tab=setup`;

	async function saved(): Promise<boolean> {
		return !ws.writable || !ws.dirty || (await ws.save());
	}

	async function go(to: number) {
		if (!(await saved())) return;
		await goto(href(steps[to]));
	}

	async function back() {
		await saved();
		await goto(href(steps[index - 1]));
	}

	async function done() {
		if (await saved()) await goto(overview());
	}

	async function fix(to: StepKey | null) {
		if (!(await saved())) return;
		await goto(to ? href(to) : `${overview()}&view=advanced`);
	}
</script>

{#if !step}
	<div class="alert alert-warning">
		<span>{t('agents-setup-unknown-step')}</span>
		<a class="btn btn-sm" href={href('start')}>{t('agents-setup-cta-start')}</a>
	</div>
{:else}
	<section class="flex flex-col rounded-box border border-base-300 bg-base-200" aria-labelledby="setup-title">
		<header class="flex flex-col gap-1 px-4 pt-4 sm:px-8">
			<nav class="text-sm text-base-content/60" aria-label={t('agents-setup-assistant')}>
				<a class="link link-hover" href={overview()}>{name}</a> › <a class="link link-hover" href={overview()}>{t('agents-tab-setup')}</a> › {t('agents-setup-assistant')}
			</nav>
			<div class="flex flex-wrap items-center gap-2">
				<h2 id="setup-title" class="m-0 text-xl font-semibold">{t(`agents-setup-step-${step}`)}</h2>
				<button class="btn btn-ghost btn-sm ml-auto" type="button" disabled={ws.busy} onclick={() => void done()}>
					<span aria-hidden="true">✕</span>{t('agents-setup-exit')}
				</button>
			</div>
			<div class="mt-2">
				<StepIndicator steps={labels} current={index} {completed} onselect={(i) => void go(i)} />
			</div>
		</header>
		<div class="px-4 pb-6 pt-3 sm:px-8">
			{#key `${step}:${ws.formKey}`}
				<fieldset disabled={!ws.writable} class="m-0 min-w-0 border-0 p-0">
					<SetupStep {step} bind:spec={ws.spec} onfix={(s) => void fix(s)} />
				</fieldset>
			{/key}
		</div>
		<footer class="sticky bottom-0 flex flex-wrap items-center gap-2.5 rounded-b-box border-t border-base-300 bg-base-200 py-3 pb-[calc(0.75rem+env(safe-area-inset-bottom))] pl-4 pr-20 sm:pl-8">
			{#if index > 0}
				<button class="btn btn-ghost" type="button" disabled={ws.busy} onclick={() => void back()}>{t('agents-setup-back')}</button>
			{/if}
			<span class="ml-auto text-sm text-base-content/60">{t('agents-setup-step-of', { current: index + 1, total: steps.length })}</span>
			{#if index < steps.length - 1}
				<button class="btn btn-primary" type="button" disabled={ws.busy} onclick={() => void go(index + 1)}>{t('agents-setup-next')}</button>
			{:else}
				<button class="btn btn-primary" type="button" disabled={ws.busy} onclick={() => void done()}>{t('agents-setup-done')}</button>
			{/if}
		</footer>
	</section>
{/if}
