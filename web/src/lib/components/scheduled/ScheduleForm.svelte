<script lang="ts">
	import { onDestroy, onMount, untrack } from 'svelte';
	import type { AgentVersion } from '#lib/agents.js';
	import { locale, t } from '#lib/i18n.svelte.js';
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import LinkedChatPicker from '#lib/components/LinkedChatPicker.svelte';
	import AgentRunPicker from '#lib/components/agents/AgentRunPicker.svelte';
	import type { OwnedAgent } from '#lib/agent-state.js';
	import { initialLinkedSession, linkedSessionField } from '#lib/linked-chat.js';
	import { modelSelectOptions, type ChatModelOption } from '#lib/model-option.js';
	import { cronFromSchedule, defaultSchedule, formatScheduledRun, scheduleFromCron, scheduledApi, scheduledVersionModel, type ScheduledAction, type ScheduledBody } from '#lib/scheduled.js';
	import { timezoneOptions } from '#lib/timezones.js';

	let { action = null, models, agents, presetAgentId = null, defaultTimezone, principalId = null, versions = [], canWrite = false, onsaved, oncancel }: {
		action?: ScheduledAction | null;
		models: ChatModelOption[];
		agents: OwnedAgent[];
		presetAgentId?: string | null;
		defaultTimezone: string | null;
		principalId?: string | null;
		versions?: AgentVersion[];
		canWrite?: boolean;
		onsaved: () => void | Promise<void>;
		oncancel?: () => void;
	} = $props();
	const initial = untrack(() => action);
	const initialAgentId = untrack(() => initial?.personal_agent_id ?? presetAgentId);
	let agentId = $state(initialAgentId ?? '');
	let name = $state(initial?.name ?? '');
	let model = $state(untrack(() => initial?.model ?? models[0]?.id ?? ''));
	let prompt = $state(initial?.prompt ?? '');
	let timezone = $state(untrack(() => initial?.timezone ?? defaultTimezone ?? ''));
	let agentVersion = $state<number | null>(initial?.agent_version ?? null);
	const organization = $derived(!!principalId);
	const authoredModel = $derived(scheduledVersionModel(versions, agentVersion));
	let toolsEnabled = $state(initial?.tools_enabled ?? true);
	let reuseConversation = $state(initial?.reuse_conversation ?? false);
	let reuseRounds = $state(initial?.reuse_rounds ?? 5);
	const initialLinked = initialLinkedSession(initial);
	let linkedSession = $state(initialLinked);
	let schedule = $state(initial ? scheduleFromCron(initial.cron) : defaultSchedule());
	let preview = $state<{ summary: string; upcoming: string[] } | null>(null);
	let error = $state<string | null>(null);
	let busy = $state(false);
	let previewVersion = 0;
	let disposed = false;
	onDestroy(() => { disposed = true; });

	const modes = ['hourly', 'daily', 'weekly', 'monthly', 'advanced'] as const;
	const weekdays = [
		[1, 'scheduled-weekday-mon'], [2, 'scheduled-weekday-tue'], [3, 'scheduled-weekday-wed'],
		[4, 'scheduled-weekday-thu'], [5, 'scheduled-weekday-fri'], [6, 'scheduled-weekday-sat'], [0, 'scheduled-weekday-sun']
	] as const;
	// Rebuilt when the zone changes so a newly picked one is present even if
	// the platform never listed it; the offsets are for "now", which is what
	// someone choosing a fire time is reasoning about.
	let timezoneChoices = $derived(timezoneOptions(timezone));
	let modelOptions = $derived(modelSelectOptions(models));

	function toggleWeekday(day: number) {
		schedule.weekdays = schedule.weekdays.includes(day)
			? schedule.weekdays.filter((candidate) => candidate !== day)
			: [...schedule.weekdays, day];
		void loadPreview();
	}

	async function loadPreview() {
		if (disposed) return;
		const cron = cronFromSchedule(schedule);
		if (!cron || !timezone.trim()) {
			preview = null;
			return;
		}
		const version = ++previewVersion;
		try {
			const result = await scheduledApi.preview(cron, timezone);
			if (!disposed && version === previewVersion) {
				preview = result;
				error = null;
			}
		} catch (caught) {
			if (!disposed && version === previewVersion) {
				preview = null;
				error = String(caught);
			}
		}
	}

	async function save() {
		if (disposed || busy) return;
		const cron = cronFromSchedule(schedule);
		if (!cron) {
			error = t(schedule.mode === 'weekly' ? 'scheduled-err-pick-weekday' : 'scheduled-err-enter-cron');
			return;
		}
		if (!canWrite) return;
		if (organization && !versions.some((version) => version.version === agentVersion)) { error = t('scheduled-organization-version-required'); return; }
		busy = true;
		error = null;
		const body: ScheduledBody = {
			name, prompt, cron, timezone, tools_enabled: toolsEnabled, reuse_rounds: reuseRounds,
			...(principalId ? { principal_id: principalId, agent_version: agentVersion!, reuse_conversation: false } : { model, personal_agent_id: agentId || null, reuse_conversation: reuseConversation, ...linkedSessionField(reuseConversation, initialLinked, linkedSession) })
		};
		try {
			if (action) await scheduledApi.update(action.id, { ...body, expected_configuration_generation: action.configuration_generation });
			else await scheduledApi.create(body);
			if (disposed) return;
			// Both paths leave for /scheduled, so there is no form left to
			// clear — the create form used to live above the list and had to
			// reset itself in place.
			await onsaved();
		} catch (caught) {
			if (!disposed) error = String(caught);
		} finally {
			if (!disposed) busy = false;
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
		{#if organization}
			{#each agents as agent}<a class="link link-hover" href={`/agents/${encodeURIComponent(agent.id)}`}>{agent.display || agent.name}</a>{/each}
			<label class="fieldset"><span class="fieldset-legend">{t('scheduled-organization-version')}</span>
				<select class="select w-full" bind:value={agentVersion} disabled={!canWrite || busy} required>
					<option value={null} disabled>{t('scheduled-organization-version-required')}</option>
					{#if agentVersion !== null && !versions.some((version) => version.version === agentVersion)}<option value={agentVersion} disabled>{t('agents-test-source-published', { version: agentVersion })}</option>{/if}
					{#each versions as version}<option value={version.version}>{t('agents-test-source-published', { version: version.version })}</option>{/each}
				</select>
			</label>
			<p class="inline-flex gap-1 text-sm"><span class="text-base-content/70">{t('scheduled-model-label')}:</span><span>{authoredModel ?? t('scheduled-organization-model-missing')}</span></p>
			<p class="text-sm text-base-content/70">{t('scheduled-organization-fresh')}</p>
		{:else}
		<label class="flex min-w-0 w-full flex-col gap-1">
			<div class="label"><span class="label-text">{t('scheduled-model-label')}</span></div>
			{#if models.length}
				<SearchableSelect options={modelOptions} bind:value={model} ariaLabel={t('scheduled-model-label')} class="w-full" />
			{:else}
				<input class="input min-w-0 w-full" bind:value={model} required aria-label={t('scheduled-model-label')} placeholder={t('scheduled-model-placeholder')} />
			{/if}
		</label>
		<AgentRunPicker {agents} initialId={initial?.personal_agent_id ?? null} bind:value={agentId} onchange={() => (linkedSession = '')} />
		{/if}
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
				<fieldset class="fieldset w-full max-w-xs min-w-0"><legend class="fieldset-legend">{t('scheduled-timezone-label')}</legend><SearchableSelect options={timezoneChoices} bind:value={timezone} ariaLabel={t('scheduled-timezone-label')} class="w-full" onchange={loadPreview} /></fieldset>
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
		{#if !organization}
		<div class="flex flex-wrap items-center gap-3">
			<label class="label cursor-pointer justify-start gap-3 whitespace-normal"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={reuseConversation} /><span>{t('scheduled-reuse-toggle-label')}</span></label>
			{#if reuseConversation}<label class="flex items-center gap-2 text-sm"><span class="opacity-70">{t('scheduled-reuse-rounds-prefix')}</span><input class="input input-sm w-20" type="number" min="1" max="50" bind:value={reuseRounds} aria-label={t('scheduled-reuse-rounds-aria')} /><span class="opacity-70">{t('scheduled-reuse-rounds-suffix')}</span></label>{/if}
		</div>
		{#if reuseConversation}<LinkedChatPicker bind:value={linkedSession} agentId={agentId || null} />{/if}
		{/if}

		<div class="card-actions justify-end">
			{#if oncancel}<button class="btn" type="button" onclick={oncancel}>{t('admin-cancel')}</button>{/if}
			<button class="btn btn-primary" type="submit" disabled={!canWrite || busy || !name.trim() || !prompt.trim() || !timezone.trim() || (organization ? !versions.some((version) => version.version === agentVersion) : !model.trim()) || !cronFromSchedule(schedule)}>{action ? t('scheduled-save-submit') : t('scheduled-create-submit')}</button>
		</div>
	</div>
</form>
