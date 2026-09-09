<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost, adminPut, adminDelete } from '$lib/admin-client';
	import { t, dt } from '$lib/i18n.svelte';

	interface Action {
		id: string;
		name: string;
		prompt: string;
		model: string;
		cron: string;
		timezone: string;
		tools_enabled: boolean;
		reuse_conversation: boolean;
		enabled: boolean;
		next_run_at: string | null;
		last_session_id: string | null;
		last_status: string | null;
	}
	interface Data {
		actions: Action[];
		models: string[];
	}

	let data = $state<Data | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let editing = $state<string | null>(null);

	let fname = $state('');
	let fprompt = $state('');
	let fmodel = $state('');
	let fcron = $state('');
	let preview = $state<string | null>(null);
	// Friendly builder state (mirrors the legacy schedule builder)
	let mode = $state<'daily' | 'weekly' | 'monthly' | 'advanced'>('daily');
	let minute = $state(0);
	let hour = $state(9);
	let weekdays = $state<boolean[]>([false, false, false, false, false, false, false]);
	let dom = $state(1);
	let advanced = $state('');
	// Keys, not labels: the day strip has to re-render on a language switch, so
	// the lookup happens in the template.
	const WEEKDAY_KEYS = [
		'scheduled-weekday-mon',
		'scheduled-weekday-tue',
		'scheduled-weekday-wed',
		'scheduled-weekday-thu',
		'scheduled-weekday-fri',
		'scheduled-weekday-sat',
		'scheduled-weekday-sun'
	];
	const MODE_KEYS: [typeof mode, string][] = [
		['daily', 'scheduled-mode-daily'],
		['weekly', 'scheduled-mode-weekly'],
		['monthly', 'scheduled-mode-monthly'],
		['advanced', 'scheduled-mode-advanced']
	];

	function assembleCron(): string | null {
		if (mode === 'advanced') return advanced.trim() || null;
		const m = Math.min(59, Math.max(0, minute));
		const h = Math.min(23, Math.max(0, hour));
		if (mode === 'daily') return `${m} ${h} * * *`;
		if (mode === 'weekly') {
			const days = weekdays.map((on, i) => (on ? i + 1 : 0)).filter(Boolean);
			if (days.length === 0) return null;
			return `${m} ${h} * * ${days.join(',')}`;
		}
		if (mode === 'monthly') return `${m} ${h} ${Math.min(31, Math.max(1, dom))} * *`;
		return null;
	}

	function buildFromCron(cron: string) {
		// Best-effort reverse parse to populate the builder from an existing row.
		const parts = cron.split(/\s+/);
		if (parts.length === 5) {
			const [m, h, domP, monP, dowP] = parts;
			minute = Number(m);
			hour = Number(h);
			if (monP === '*' && domP === '*' && dowP === '*') {
				mode = 'daily';
			} else if (monP === '*' && domP === '*' && dowP !== '*') {
				mode = 'weekly';
				weekdays = [false, false, false, false, false, false, false];
				for (const d of dowP.split(',')) weekdays[Number(d) - 1] = true;
			} else if (monP === '*' && dowP === '*' && domP !== '*') {
				mode = 'monthly';
				dom = Number(domP);
			} else {
				mode = 'advanced';
				advanced = cron;
			}
		} else {
			mode = 'advanced';
			advanced = cron;
		}
	}

	async function refresh() {
		try {
			data = await adminJson<Data>('/api/v0/scheduled');
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function runPreview() {
		if (!fcron.trim()) {
			preview = null;
			return;
		}
		try {
			const res = await adminPost<{ summary: string; upcoming: string[] }>(
				'/api/v0/scheduled/preview',
				{ cron: fcron }
			);
			// `scheduled-next-run` rather than the `-prefix` key: a whole
			// sentence with the list in a placeholder, so a translator can move
			// the list where their language wants it. A prefix concatenated
			// with a value fixes the word order to English's.
			const when = res.upcoming.map((u) => dt(u)).join(', ');
			preview = `${res.summary} — ${t('scheduled-next-run', { when })}`;
		} catch (err) {
			preview = String(err);
		}
	}

	async function save() {
		notice = null;
		try {
			const body = {
				name: fname,
				prompt: fprompt,
				model: fmodel,
				tools_enabled: false,
				reuse_conversation: false,
				reuse_rounds: 5
			};
			const cron = assembleCron();
			if (!cron) {
				notice = t('scheduled-err-invalid-schedule');
				return;
			}
			if (editing) await adminPut(`/api/v0/scheduled/${editing}`, { ...body, cron });
			else await adminPost('/api/v0/scheduled', { ...body, cron });
			editing = null;
			fname = ''; fprompt = ''; fcron = '';
			preview = null;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function toggle(a: Action) {
		try {
			await adminPost(`/api/v0/scheduled/${a.id}/toggle`, { enabled: !a.enabled });
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function remove(id: string) {
		if (!confirm(t('scheduled-delete-confirm'))) return;
		try {
			await adminDelete(`/api/v0/scheduled/${id}`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">{t('scheduled-heading')}</h1>
</div>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

<div class="card border border-base-300 mb-6">
	<div class="card-body">
		<h2 class="card-title text-base">
			{editing ? t('scheduled-edit-heading') : t('scheduled-new-heading')}
		</h2>
		<div class="grid sm:grid-cols-2 gap-3">
			<label class="flex flex-col gap-1"><span class="label-text">{t('scheduled-name-label')}</span>
				<input
					class="input input-bordered input-sm"
					bind:value={fname}
					placeholder={t('scheduled-name-placeholder')}
				/></label>
			<label class="flex flex-col gap-1"><span class="label-text">{t('scheduled-model-label')}</span>
				<input
					class="input input-bordered input-sm"
					bind:value={fmodel}
					list="sched-models"
					placeholder={t('scheduled-model-placeholder')}
				/>
				<datalist id="sched-models">{#each data?.models ?? [] as m (m)}<option value={m}></option>{/each}</datalist>
			</label>
			<label class="flex flex-col gap-1 sm:col-span-2"><span class="label-text">{t('scheduled-prompt-label')}</span>
				<textarea
					class="textarea textarea-bordered text-sm"
					rows="3"
					bind:value={fprompt}
					placeholder={t('scheduled-prompt-placeholder')}
				></textarea></label>
			<label class="flex flex-col gap-1">
				<span class="label-text">{t('scheduled-builder-heading')}</span>
				<div class="flex gap-1">
					{#each MODE_KEYS as [m, label] (m)}
						<button
							class="btn btn-xs {mode === m ? 'btn-primary' : 'btn-ghost'}"
							onclick={() => (mode = m)}
						>{t(label)}</button>
					{/each}
				</div>
			</label>
			{#if mode === 'daily'}
				<label class="flex flex-col gap-1 w-24"><span class="label-text">{t('scheduled-hour-aria')}</span>
					<input type="number" min="0" max="23" class="input input-bordered input-sm" bind:value={hour} /></label>
				<label class="flex flex-col gap-1 w-24"><span class="label-text">{t('scheduled-minute-aria')}</span>
					<input type="number" min="0" max="59" class="input input-bordered input-sm" bind:value={minute} /></label>
			{:else if mode === 'weekly'}
				<div class="flex flex-col gap-1"><span class="label-text">{t('scheduled-weekdays-label')}</span>
					<div class="flex gap-1">
						{#each WEEKDAY_KEYS as day, i (day)}
							<button
								class="btn btn-xs {weekdays[i] ? 'btn-primary' : 'btn-ghost'}"
								onclick={() => (weekdays[i] = !weekdays[i])}
							>{t(day)}</button>
						{/each}
					</div>
				</div>
				<label class="flex flex-col gap-1 w-24"><span class="label-text">{t('scheduled-hour-aria')}</span>
					<input type="number" min="0" max="23" class="input input-bordered input-sm" bind:value={hour} /></label>
				<label class="flex flex-col gap-1 w-24"><span class="label-text">{t('scheduled-minute-aria')}</span>
					<input type="number" min="0" max="59" class="input input-bordered input-sm" bind:value={minute} /></label>
			{:else if mode === 'monthly'}
				<label class="flex flex-col gap-1 w-28"><span class="label-text">{t('scheduled-day-of-month-label')}</span>
					<input type="number" min="1" max="31" class="input input-bordered input-sm" bind:value={dom} /></label>
				<label class="flex flex-col gap-1 w-24"><span class="label-text">{t('scheduled-hour-aria')}</span>
					<input type="number" min="0" max="23" class="input input-bordered input-sm" bind:value={hour} /></label>
				<label class="flex flex-col gap-1 w-24"><span class="label-text">{t('scheduled-minute-aria')}</span>
					<input type="number" min="0" max="59" class="input input-bordered input-sm" bind:value={minute} /></label>
			{:else}
				<label class="flex flex-col gap-1"><span class="label-text">{t('scheduled-cron-label')}</span>
					<input class="input input-bordered input-sm font-mono" bind:value={advanced} oninput={() => setTimeout(runPreview, 400)} placeholder="0 9 * * 1-5" /></label>
			{/if}
			<button
				class="btn btn-ghost btn-xs self-end"
				onclick={() => {
					const c = assembleCron();
					if (c) {
						fcron = c;
						void runPreview();
					} else preview = t('scheduled-err-pick-weekday');
				}}
			>{t('scheduled-update-preview')}</button>
			{#if preview}<p class="text-xs text-base-content/60 self-end">{preview}</p>{/if}
		</div>
		<div class="card-actions justify-end mt-2">
			{#if editing}<button class="btn btn-ghost btn-sm" onclick={() => { editing = null; fname=''; fprompt=''; fcron=''; }}>{t('admin-cancel')}</button>{/if}
			<button class="btn btn-primary btn-sm" onclick={save} disabled={!fname.trim() || !fprompt.trim() || !fmodel.trim() || (mode === 'advanced' && !advanced.trim()) || (mode === 'weekly' && !weekdays.some(Boolean))}>{t('groups-save')}</button>
		</div>
	</div>
</div>

<ul class="flex flex-col gap-3">
	{#each data?.actions ?? [] as a (a.id)}
		<li class="card border border-base-300">
			<div class="card-body py-3">
				<div class="flex items-center gap-3 flex-wrap">
					{#if a.enabled}<span class="badge badge-success badge-sm">{t('scheduled-badge-active')}</span>{:else}<span class="badge badge-ghost badge-sm">{t('scheduled-badge-paused')}</span>{/if}
					<span class="font-medium">{a.name}</span>
					<span class="font-mono text-xs text-base-content/50">{a.cron}</span>
					{#if a.next_run_at}
						<span class="text-xs text-base-content/60">
							{t('scheduled-next-run', { when: dt(a.next_run_at) })}
						</span>
					{/if}
					{#if a.last_status === 'error'}<span class="badge badge-error badge-sm">{t('scheduled-badge-last-run-failed')}</span>{/if}
					<span class="flex-1"></span>
					<button class="btn btn-ghost btn-xs" onclick={() => toggle(a)}>{a.enabled ? t('scheduled-pause-title') : t('scheduled-resume-title')}</button>
					<button class="btn btn-ghost btn-xs" onclick={() => { editing = a.id; fname = a.name; fprompt = a.prompt; fmodel = a.model; buildFromCron(a.cron); }}>{t('scheduled-edit-title')}</button>
					<button class="btn btn-ghost btn-xs text-error" onclick={() => remove(a.id)}>{t('scheduled-delete-title')}</button>
				</div>
				<p class="text-xs text-base-content/60 line-clamp-2">{a.prompt}</p>
			</div>
		</li>
	{:else}
		<li class="text-sm text-base-content/50">{t('scheduled-list-empty')}</li>
	{/each}
</ul>
