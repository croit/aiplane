<script lang="ts">
	import { onMount } from 'svelte';
	import DotAssociation from '#lib/components/dots/DotAssociation.svelte';
	import { page } from '$app/state';
	import { runLinks } from '#lib/run-links.js';
	import { locale, t } from '#lib/i18n.svelte.js';
	import { scheduledApi, formatScheduledRun, type ScheduledAction, type ScheduledRun } from '#lib/scheduled.js';

	/**
	 * One schedule's run history — the list of chats it produced.
	 *
	 * Mirrors `/webhooks/{id}/runs`: the schedule's `last_*` summary on the
	 * list row answers "did the most recent one work", this answers "what has
	 * it been writing". Each run links into the conversation it opened; a
	 * schedule that reuses its conversation points every run at the same one,
	 * which is the honest picture rather than a bug.
	 */
	let action = $state<ScheduledAction | null>(null);
	let runs = $state<ScheduledRun[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	const listHref = $derived(`/scheduled${action?.principal_id || page.url.searchParams.get('principal_id') ? `?principal_id=${encodeURIComponent(action?.principal_id ?? page.url.searchParams.get('principal_id')!)}` : ''}`);

	function runTime(value: string): string {
		return formatScheduledRun(value, locale.current, action?.timezone ?? 'UTC');
	}

	onMount(async () => {
		try {
			const data = await scheduledApi.runs(page.params.id!);
			action = data.action;
			runs = data.runs;
			error = null;
		} catch (caught) {
			error = String(caught);
		} finally {
			loading = false;
		}
	});
</script>

<svelte:head><title>{t('scheduled-runs-page-title')}</title></svelte:head>

<div class="w-full max-w-4xl">
	<a class="link link-hover text-sm text-base-content/60" href={listHref}>← {t('scheduled-back')}</a>
	<h1 class="m-0 mt-2 text-2xl font-bold">{action ? t('scheduled-runs-heading', { name: action.name }) : t('scheduled-runs-page-title')}</h1>
	<p class="mt-2 text-sm text-base-content/60">{t('scheduled-runs-intro')}</p>

	{#if error}<div class="alert alert-error mt-4 text-sm"><span>{error}</span></div>{/if}

	{#if loading}
		<div class="skeleton mt-5 h-40 w-full"></div>
	{:else if action}
		<DotAssociation binding={action} />
		<section class="card card-border mt-5">
			<div class="card-body">
				<ul class="flex flex-col divide-y divide-base-300">
					{#each runs as run (run.id)}
						<li class="flex flex-col gap-2 py-3 sm:flex-row sm:items-start">
							<div class="min-w-0 flex-1">
								<div class="flex flex-wrap items-center gap-2">
									<strong class="text-sm">{runTime(run.fired_at)}</strong>
									<span class="badge badge-sm {run.status === 'ok' ? 'badge-success' : run.status === 'error' ? 'badge-error' : run.status === 'waiting' ? 'badge-warning' : 'badge-ghost'}">
										{t(run.status === 'ok' ? 'scheduled-run-status-ok' : run.status === 'error' ? 'scheduled-run-status-error' : run.status === 'waiting' ? 'dot-background-waiting' : 'scheduled-run-status-pending')}
									</span>
								</div>
								{#if run.agent_version}<p class="text-xs text-base-content/60">{t('agents-test-source-published', { version: run.agent_version })}</p>{/if}
								{#if run.error}<p class="mt-0.5 text-xs text-error">{run.error}</p>{/if}
							</div>
							<div class="flex shrink-0 items-center gap-3 self-end text-sm sm:self-auto">
								{#if run.status === 'waiting'}<a class="link link-hover" href="/inbox">{t('inbox-heading')}</a>{/if}
								{#if run.chat_deleted}
									<span class="badge badge-sm badge-outline text-base-content/60">{t('scheduled-run-chat-deleted')}</span>
								{:else if run.session_id}
									<a class="link link-hover" href={runLinks({ id: run.id, last_session_id: run.session_id, run_count: 1, chat_count: 1 }, '/scheduled', { principalId: action.principal_id }).chat!}>{t('scheduled-run-open')}</a>
								{/if}
							</div>
						</li>
					{:else}
						<li class="py-2 text-sm text-base-content/60">{t('scheduled-runs-empty')}</li>
					{/each}
				</ul>
			</div>
		</section>
	{/if}
</div>
