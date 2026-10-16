<script lang="ts">
	import type { AdminLimitRule, AdminLimitsData, LimitDimension, LimitSubjectType, LimitWindow } from '#lib/admin-limits.js';
	import { limitSubjectName } from '#lib/admin-limits.js';
	import { t } from '#lib/i18n.svelte.js';
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';

	let { data, editing = null, onsave }: { data: AdminLimitsData; editing?: AdminLimitRule | null; onsave: (body: Record<string, string | number | boolean>, subject: string) => Promise<void> } = $props();
	let subjectType = $state<LimitSubjectType>('global');
	let subjectId = $state('');
	let model = $state('');
	let dimension = $state<LimitDimension>('requests');
	let window = $state<LimitWindow>('hour');
	let value = $state('');
	let isTotal = $state(false);
	let saving = $state(false);
	let subjectOptions = $derived([
		{ value: '', label: t('limits-field-subject-id-ph') },
		...(subjectType === 'role' ? data.roles.map((role) => ({ value: role, label: role })) : []),
		...(subjectType === 'user' ? data.users.map((user) => ({ value: user.id, label: user.email, keywords: [user.id] })) : []),
		...(subjectType === 'token' ? data.tokens.map((token) => ({ value: token.id, label: token.name, description: token.owner, keywords: [token.id] })) : []),
		...(subjectType === 'system' ? data.agents.map((agent) => ({ value: agent.id, label: agent.name, keywords: [agent.id] })) : [])
	]);
	let modelOptions = $derived([{ value: '', label: t('limits-all-models') }, ...data.models.map((candidate) => ({ value: candidate, label: candidate }))]);
	$effect(() => {
		subjectType = editing?.subject_type ?? 'global';
		subjectId = editing?.subject_id ?? '';
		model = editing?.model ?? '';
		dimension = editing?.dimension ?? 'requests';
		window = editing?.window ?? 'hour';
		value = editing ? String(editing.value) : '';
		isTotal = editing?.is_total ?? false;
	});

	function changeSubjectType(event: Event) {
		subjectType = (event.currentTarget as HTMLSelectElement).value as LimitSubjectType;
		subjectId = '';
	}

	async function submit() {
		saving = true;
		try {
			const subject = subjectType === 'global' ? t('limits-subject-global') : limitSubjectName({ subject_type: subjectType, subject_id: subjectId }, data);
			await onsave({ ...(editing ? { id: editing.id } : {}), subject_type: subjectType, subject_id: subjectId, model, dimension, window, value: Number(value), is_total: isTotal }, subject);
		} finally { saving = false; }
	}
</script>

<article class="card">
	<div class="card-body gap-3">
		<h2 class="card-title text-base">{t(editing ? 'limits-edit-heading' : 'limits-add-heading')}</h2>
		<form class="m-0 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3" onsubmit={(event) => { event.preventDefault(); void submit(); }}>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-subject-type">{t('limits-field-subject')}</label><select id="limit-subject-type" class="select w-full" value={subjectType} onchange={changeSubjectType}><option value="global">{t('limits-subject-global')}</option><option value="role">{t('limits-subject-role')}</option><option value="user">{t('limits-subject-user')}</option><option value="token">{t('limits-subject-token')}</option><option value="system">{t('limits-subject-system')}</option></select></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-subject-id">{t('limits-field-subject-id')}</label><SearchableSelect id="limit-subject-id" options={subjectOptions} bind:value={subjectId} ariaLabel={t('limits-field-subject-id')} class="w-full" disabled={subjectType === 'global'} /></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-model">{t('limits-field-model')}</label><input id="limit-model" class="input w-full font-mono" type="text" list="limit-model-list" bind:value={model} placeholder={t('limits-all-models')} /><datalist id="limit-model-list">{#each modelOptions as option}<option value={option.value}>{option.label}</option>{/each}</datalist><span class="text-xs opacity-70">{t('limits-field-model-hint')}</span></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-dimension">{t('limits-field-dimension')}</label><select id="limit-dimension" class="select w-full" bind:value={dimension}><option value="requests">{t('limits-dim-requests')}</option><option value="tokens">{t('limits-dim-tokens')}</option><option value="cost">{t('limits-dim-cost', { cur: data.currency })}</option></select></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-window">{t('limits-field-window')}</label><select id="limit-window" class="select w-full" bind:value={window}><option value="hour">{t('limits-win-hour')}</option><option value="day">{t('limits-win-day')}</option><option value="week">{t('limits-win-week')}</option><option value="month">{t('limits-win-month')}</option></select></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-value">{t('limits-field-value')}</label><input id="limit-value" class="input w-full" type="number" min="0" step="any" required bind:value={value} /></div>
			<div class="flex items-end"><button class="btn btn-primary" type="submit" disabled={saving || !value || (subjectType !== 'global' && !subjectId)}>{t(editing ? 'limits-edit-submit' : 'limits-add-submit')}</button></div>
			<label class="flex cursor-pointer items-start gap-2 sm:col-span-2 lg:col-span-3"><input id="limit-is-total" type="checkbox" class="checkbox mt-1" bind:checked={isTotal} /><span><span class="label-text">{t('limits-field-total')}</span><br /><span class="text-xs opacity-70">{t('limits-field-total-hint')}</span></span></label>
		</form>
	</div>
</article>
