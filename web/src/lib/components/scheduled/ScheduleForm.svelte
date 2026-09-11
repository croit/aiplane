<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import { adminPost, adminPut } from '$lib/admin-client';
	import { locale, t } from '$lib/i18n.svelte';
	import type { ChatModelOption } from '$lib/model-option';
	import { cronFromSchedule, defaultSchedule, formatScheduledRun, scheduleFromCron, type ScheduledAction } from '$lib/scheduled';

	let { action = null, models, defaultTimezone, onsaved, oncancel } = $props<{
		action?: ScheduledAction | null;
		models: ChatModelOption[];
		defaultTimezone: string;
		onsaved: () => void | Promise<void>;
		oncancel?: () => void;
	}>();
	const initial = untrack(() => action);
	let name = $state(initial?.name ?? '');
	let model = $state(untrack(() => initial?.model ?? models[0]?.id ?? ''));
	let prompt = $state(initial?.prompt ?? '');
	let timezone = $state(untrack(() => initial?.timezone ?? defaultTimezone));
	let toolsEnabled = $state(initial?.tools_enabled ?? true);
	let reuseConversation = $state(initial?.reuse_conversation ?? false);
	let reuseRounds = $state(initial?.reuse_rounds ?? 5);
	let schedule = $state(initial ? scheduleFromCron(initial.cron) : defaultSchedule());
	let preview = $state<{ summary: string; upcoming: string[] } | null>(null);
	let error = $state<string | null>(null);
	let busy = $state(false);
	let previewVersion = 0;

	const modes = ['hourly', 'daily', 'weekly', 'monthly', 'advanced'] as const;
	const weekdays = [
		[1, 'scheduled-weekday-mon'], [2, 'scheduled-weekday-tue'], [3, 'scheduled-weekday-wed'],
		[4, 'scheduled-weekday-thu'], [5, 'scheduled-weekday-fri'], [6, 'scheduled-weekday-sat'], [0, 'scheduled-weekday-sun']
	] as const;
	let selectedModel = $derived(models.find((candidate: ChatModelOption) => candidate.id === model));
	function modelLabel(candidate: ChatModelOption): string {
		if (!candidate.gdpr && !candidate.nda) return t('scheduled-model-non-gdpr-nda-restricted', { model: candidate.id });
		if (!candidate.gdpr) return t('scheduled-model-non-gdpr', { model: candidate.id });
		if (!candidate.nda) return t('scheduled-model-nda-restricted', { model: candidate.id });
		return candidate.id;
	}

	function toggleWeekday(day: number) {
		schedule.weekdays = schedule.weekdays.includes(day)
			? schedule.weekdays.filter((candidate) => candidate !== day)
			: [...schedule.weekdays, day];
		void loadPreview();
	}

	async function loadPreview() {
		const cron = cronFromSchedule(schedule);
		if (!cron || !timezone.trim()) {
			preview = null;
			return;
		}
		const version = ++previewVersion;
		try {
			const result = await adminPost<{ summary: string; upcoming: string[] }>('/api/v0/scheduled/preview', { cron, timezone });
			if (version === previewVersion) {
				preview = result;
				error = null;
			}
		} catch (caught) {
			if (version === previewVersion) {
				preview = null;
				error = String(caught);
			}
		}
	}

	async function save() {
		const cron = cronFromSchedule(schedule);
		if (!cron) {
			error = t(schedule.mode === 'weekly' ? 'scheduled-err-pick-weekday' : 'scheduled-err-enter-cron');
			return;
		}
		busy = true;
		error = null;
		const body = {
			name, prompt, model, cron, timezone, tools_enabled: toolsEnabled,
			reuse_conversation: reuseConversation, reuse_rounds: reuseRounds
		};
		try {
			if (action) await adminPut(`/api/v0/scheduled/${action.id}`, body);
			else await adminPost('/api/v0/scheduled', body);
			await onsaved();
			if (!action) {
				name = '';
				prompt = '';
				schedule = defaultSchedule();
				toolsEnabled = true;
				reuseConversation = false;
				reuseRounds = 5;
				await loadPreview();
			}
		} catch (caught) {
			error = String(caught);
		} finally {
			busy = false;
		}
	}

	onMount(loadPreview);
</script>

<form class="card mb-6 min-w-0 border border-base-300" onsubmit={(event) => { event.preventDefault(); void save(); }}>
	<div class="card-body min-w-0 gap-4">
		{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
		<label class="flex min-w-0 w-full flex-col gap-1">
			<div class="label"><span class="label-text">{t('scheduled-name-label')}</span></div>
			<input class="input min-w-0 w-full" bind:value={name} maxlength="128" required aria-label={t('scheduled-name-label')} placeholder={t('scheduled-name-placeholder')} />
		</label>
		<label class="flex min-w-0 w-full flex-col gap-1">
			<div class="label"><span class="label-text">{t('scheduled-model-label')}</span></div>
			{#if models.length}
				<select class="select min-w-0 max-w-full w-full" bind:value={model} aria-label={t('scheduled-model-label')}>
					{#each models as candidate (candidate.id)}
						<option value={candidate.id}>{modelLabel(candidate)}</option>
					{/each}
				</select>
			{:else}
				<input class="input min-w-0 w-full" bind:value={model} required aria-label={t('scheduled-model-label')} placeholder={t('scheduled-model-placeholder')} />
			{/if}
		</label>
		{#if selectedModel && !selectedModel.gdpr}<div class="alert alert-error"><span>{t('scheduled-gdpr-warning')}</span></div>{/if}
		{#if selectedModel && !selectedModel.nda}<div class="alert alert-error"><span>{t('scheduled-nda-warning')}</span></div>{/if}
		<label class="flex min-w-0 w-full flex-col gap-1">
			<div class="label"><span class="label-text">{t('scheduled-prompt-label')}</span></div>
			<textarea class="textarea min-h-28 min-w-0 max-w-full w-full" bind:value={prompt} maxlength="8000" required aria-label={t('scheduled-prompt-label')} placeholder={t('scheduled-prompt-placeholder')}></textarea>
		</label>

		<div class="flex min-w-0 flex-col gap-4 rounded-lg border border-base-300 p-4">
			<div class="text-sm font-medium">{t('scheduled-builder-heading')}</div>
			<div class="flex flex-wrap gap-2">
				{#each modes as mode}
					<label class="label cursor-pointer gap-2 rounded-md border border-base-300 px-3 py-1.5 whitespace-normal">
						<input class="radio radio-sm" type="radio" name={`mode-${action?.id ?? 'new'}`} value={mode} bind:group={schedule.mode} onchange={loadPreview} />
						<span>{t(`scheduled-mode-${mode}`)}</span>
					</label>
				{/each}
			</div>

			{#if schedule.mode === 'weekly'}
				<div class="flex flex-wrap gap-1">
					{#each weekdays as [day, label]}
						<label class="label cursor-pointer gap-1.5 rounded-md border border-base-300 px-2.5 py-1 whitespace-normal">
							<input class="checkbox checkbox-xs" type="checkbox" checked={schedule.weekdays.includes(day)} onchange={() => toggleWeekday(day)} />
							<span class="text-xs">{t(label)}</span>
						</label>
					{/each}
				</div>
			{/if}

			{#if schedule.mode === 'monthly'}
				<div class="flex items-end gap-2">
					<fieldset class="fieldset"><legend class="fieldset-legend">{t('scheduled-on-day-label')}</legend><input class="input w-24" type="number" min="1" max="31" bind:value={schedule.dayOfMonth} onchange={loadPreview} aria-label={t('scheduled-on-day-label')} /></fieldset>
					<span class="pb-3 text-base-content/60">{t('scheduled-of-every-month')}</span>
				</div>
			{/if}

			<div class="flex flex-wrap items-end gap-6">
				{#if schedule.mode !== 'advanced'}
					<fieldset class="fieldset"><legend class="fieldset-legend">{t('scheduled-at-label')}</legend>
						<div class="flex items-center gap-1">
							{#if schedule.mode !== 'hourly'}<input class="input w-20" type="number" min="0" max="23" bind:value={schedule.hour} onchange={loadPreview} aria-label={t('scheduled-hour-aria')} /><strong>:</strong>{/if}
							<input class="input w-20" type="number" min="0" max="59" bind:value={schedule.minute} onchange={loadPreview} aria-label={t('scheduled-minute-aria')} />
							{#if schedule.mode === 'hourly'}<span class="ml-1 text-base-content/60">{t('scheduled-of-every-hour')}</span>{/if}
						</div>
					</fieldset>
				{/if}
				<fieldset class="fieldset w-full max-w-xs min-w-0"><legend class="fieldset-legend">{t('scheduled-timezone-label')}</legend><input class="input min-w-0 w-full" bind:value={timezone} onchange={loadPreview} aria-label={t('scheduled-timezone-label')} placeholder={t('scheduled-timezone-placeholder')} /></fieldset>
			</div>

			{#if schedule.mode === 'advanced'}
				<fieldset class="fieldset min-w-0"><legend class="fieldset-legend">{t('scheduled-cron-label')}</legend><input class="input min-w-0 w-full font-mono" bind:value={schedule.advanced} onchange={loadPreview} aria-label={t('scheduled-cron-label')} placeholder="0 9 * * *" /><p class="label whitespace-normal">{t('scheduled-cron-help')}</p></fieldset>
			{/if}

			<div class="flex flex-col gap-1 text-sm">
				{#if preview}
					<p class="font-medium">{t('scheduled-preview-summary', { summary: preview.summary, timezone })}</p>
					<p class="text-base-content/70">{t('scheduled-preview-next-runs', { runs: preview.upcoming.map((run) => formatScheduledRun(run, locale.current, timezone)).join(' · ') })}</p>
				{:else}<p class="text-base-content/60">{t('scheduled-no-upcoming-runs')}</p>{/if}
			</div>
		</div>

		<label class="label cursor-pointer justify-start gap-3 whitespace-normal"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={toolsEnabled} /><span>{t('scheduled-tools-toggle-label')}</span></label>
		<div class="flex flex-wrap items-center gap-3">
			<label class="label cursor-pointer justify-start gap-3 whitespace-normal"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={reuseConversation} /><span>{t('scheduled-reuse-toggle-label')}</span></label>
			{#if reuseConversation}<label class="flex items-center gap-2 text-sm"><span class="opacity-70">{t('scheduled-reuse-rounds-prefix')}</span><input class="input input-sm w-20" type="number" min="1" max="50" bind:value={reuseRounds} aria-label={t('scheduled-reuse-rounds-aria')} /><span class="opacity-70">{t('scheduled-reuse-rounds-suffix')}</span></label>{/if}
		</div>

		<div class="card-actions justify-end">
			{#if oncancel}<button class="btn" type="button" onclick={oncancel}>{t('admin-cancel')}</button>{/if}
			<button class="btn btn-primary" type="submit" disabled={busy || !name.trim() || !prompt.trim() || !model.trim() || !cronFromSchedule(schedule)}>{action ? t('scheduled-save-submit') : t('scheduled-create-submit')}</button>
		</div>
	</div>
</form>
