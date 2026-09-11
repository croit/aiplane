<script lang="ts">
	import { adminDelete, adminPost } from '$lib/admin-client';
	import { locale, t } from '$lib/i18n.svelte';
	import { formatScheduledRun, type ScheduledAction } from '$lib/scheduled';

	let { action, onchanged, onnotice } = $props<{
		action: ScheduledAction;
		onchanged: () => void | Promise<void>;
		onnotice: (message: string) => void;
	}>();

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

<li class="flex flex-col gap-3 py-3 sm:flex-row sm:items-start">
	<div class="min-w-0 flex-1">
		<div class="flex flex-wrap items-center gap-2"><strong class="text-sm">{action.name}</strong><span class="badge badge-sm {action.enabled ? 'badge-success' : 'badge-ghost'}">{action.enabled ? t('scheduled-badge-active') : t('scheduled-badge-paused')}</span></div>
		<p class="truncate text-xs text-base-content/60">{action.prompt}</p>
		<p class="mt-0.5 text-xs text-base-content/70">{action.model} · {action.schedule_summary} ({action.timezone})</p>
		<div class="mt-0.5 flex flex-wrap items-center gap-x-3 text-xs text-base-content/60">
			<span>{action.enabled ? (action.next_run_at ? t('scheduled-next-run', { when: runTime(action.next_run_at) }) : t('scheduled-no-upcoming-run')) : t('scheduled-status-paused')}</span>
			{#if action.last_run_at}
				{#if action.last_session_id}<a class="link link-hover {action.last_status === 'ok' ? 'text-success' : 'text-error'}" href={`/chat/${action.last_session_id}`}>{t(action.last_status === 'ok' ? 'scheduled-last-success-open' : 'scheduled-last-failure-open', { when: runTime(action.last_run_at) })}</a>
				{:else}<span class={action.last_status === 'ok' ? 'text-success' : 'text-error'}>{t(action.last_status === 'ok' ? 'scheduled-last-success' : 'scheduled-last-failure', { when: runTime(action.last_run_at) })}</span>{/if}
			{/if}
		</div>
		{#if action.last_error}<p class="mt-1 text-xs text-error">{t('scheduled-last-error', { error: action.last_error })}</p>{/if}
	</div>
	<div class="flex shrink-0 items-center gap-1 self-end sm:self-auto">
		<button class="btn btn-ghost btn-sm" type="button" onclick={toggle}>{action.enabled ? t('scheduled-pause-title') : t('scheduled-resume-title')}</button>
		<a class="btn btn-ghost btn-sm" href={`/scheduled/${action.id}/edit`}>{t('scheduled-edit-title')}</a>
		<button class="btn btn-ghost btn-error btn-sm" type="button" onclick={remove}>{t('scheduled-delete-title')}</button>
	</div>
</li>
