<script lang="ts">
	import { adminDelete, adminPost } from '#lib/admin-client.js';
	import { locale, t } from '#lib/i18n.svelte.js';
	import DotAssociation from '#lib/components/dots/DotAssociation.svelte';
	import { runLinks } from '#lib/run-links.js';
	import { formatScheduledRun, type ScheduledAction } from '#lib/scheduled.js';

	/**
	 * One schedule on `/scheduled`.
	 *
	 * The card answers the two questions the page exists for: when does this
	 * run next, and where is what it produced. The second one used to be a
	 * single "open" link on the last run — everything the schedule had written
	 * before that was unreachable from here. `scheduleLinks` picks between the
	 * one conversation and the list of them.
	 */
	let { action, onchanged, onnotice } = $props<{
		action: ScheduledAction;
		onchanged: () => void | Promise<void>;
		onnotice: (message: string) => void;
	}>();

	let links = $derived(runLinks(action, '/scheduled'));

	function runTime(value: string): string {
		return formatScheduledRun(value, locale.current, action.timezone);
	}

	async function toggle() {
		try {
			await adminPost(`/api/v0/scheduled/${action.id}/toggle`, { enabled: !action.enabled });
			await onchanged();
		} catch (caught) {
			onnotice(String(caught));
		}
	}

	async function remove() {
		if (!confirm(t('scheduled-delete-confirm'))) return;
		try {
			await adminDelete(`/api/v0/scheduled/${action.id}`);
			await onchanged();
		} catch (caught) {
			onnotice(String(caught));
		}
	}
</script>

<article class="card card-border">
	<div class="card-body gap-4">
		<div class="flex flex-col gap-3 sm:flex-row sm:items-start">
			<div class="min-w-0 flex-1">
				<div class="flex flex-wrap items-center gap-2">
					<h3 class="card-title text-base">{action.name}</h3>
					<span class="badge badge-sm {action.enabled ? 'badge-success' : 'badge-ghost'}">{action.enabled ? t('scheduled-badge-active') : t('scheduled-badge-paused')}</span>
					{#if action.reuse_conversation}<span class="badge badge-ghost badge-sm">{t('scheduled-badge-reuses-chat')}</span>{/if}
				</div>
				<DotAssociation binding={action} />
				<p class="mt-1 line-clamp-2 text-sm text-base-content/70">{action.prompt}</p>
				<div class="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-xs text-base-content/60">
					<span class="font-mono">{action.model}</span>
					<span>·</span>
					<span>{action.schedule_summary} ({action.timezone})</span>
					<span>·</span>
					<span>{action.enabled ? (action.next_run_at ? t('scheduled-next-run', { when: runTime(action.next_run_at) }) : t('scheduled-no-upcoming-run')) : t('scheduled-status-paused')}</span>
				</div>
				{#if action.last_run_at}
					<p class="mt-1 text-xs {action.last_status === 'ok' ? 'text-success' : action.last_status === 'waiting' ? 'text-warning' : 'text-error'}">
						{t(action.last_status === 'ok' ? 'scheduled-last-success' : action.last_status === 'waiting' ? 'dot-background-last-waiting' : 'scheduled-last-failure', { when: runTime(action.last_run_at) })}
					</p>
				{/if}
				{#if action.last_status === 'waiting'}<a class="link link-hover text-sm" href="/inbox">{t('inbox-heading')}</a>{/if}
				{#if action.last_error}<p class="mt-1 text-xs text-error">{t('scheduled-last-error', { error: action.last_error })}</p>{/if}
			</div>
			<div class="flex flex-wrap gap-2">
				<a class="btn btn-sm" href="/scheduled/{action.id}/edit">{t('scheduled-edit-title')}</a>
				<button class="btn btn-sm" type="button" onclick={toggle}>{action.enabled ? t('scheduled-pause-title') : t('scheduled-resume-title')}</button>
				<button class="btn btn-outline btn-error btn-sm" type="button" onclick={remove}>{t('scheduled-delete-title')}</button>
			</div>
		</div>

		<!-- What this schedule produced. A schedule that has never run says so
		     rather than showing two dead links. -->
		<div class="flex flex-wrap items-center gap-3 border-t border-base-300 pt-3 text-sm">
			{#if links.chat}
				<a class="link link-hover font-medium" href={links.chat}>{t('scheduled-open-chat')}</a>
				{#if links.runs}<a class="link link-hover text-base-content/70" href={links.runs}>{t('scheduled-open-runs', { count: action.run_count })}</a>{/if}
			{:else if links.runs}
				{#if action.last_chat_deleted && action.chat_count <= 1}<span class="badge badge-sm badge-outline text-base-content/60">{t('scheduled-run-chat-deleted')}</span>{/if}
				<a class="link link-hover font-medium" href={links.runs}>
					{action.chat_count > 0 ? t('scheduled-open-chats', { count: action.chat_count }) : t('scheduled-open-runs', { count: action.run_count })}
				</a>
			{:else}
				<span class="text-base-content/50">{t('scheduled-never-run')}</span>
			{/if}
		</div>
	</div>
</article>
