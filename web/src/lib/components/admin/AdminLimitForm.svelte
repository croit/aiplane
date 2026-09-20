<script lang="ts">
	import type { AdminLimitRule, AdminLimitsData, LimitDimension, LimitSubjectType, LimitWindow } from '$lib/admin-limits';
	import { limitSubjectName } from '$lib/admin-limits';
	import { t } from '$lib/i18n.svelte';
	import SearchableSelect from '$lib/components/SearchableSelect.svelte';

	let { data, editing = null, onsave }: { data: AdminLimitsData; editing?: AdminLimitRule | null; onsave: (body: Record<string, string | number>, subject: string) => Promise<void> } = $props();
	let subjectType = $state<LimitSubjectType>('global');
	let subjectId = $state('');
	let model = $state('');
	let dimension = $state<LimitDimension>('requests');
	let window = $state<LimitWindow>('hour');
	let value = $state('');
	let saving = $state(false);
	let subjectOptions = $derived([
		{ value: '', label: t('limits-field-subject-id-ph') },
		...(subjectType === 'role' ? data.roles.map((role) => ({ value: role, label: role })) : []),
		...(subjectType === 'user' ? data.users.map((user) => ({ value: user.id, label: user.email, keywords: [user.id] })) : []),
		...(subjectType === 'token' ? data.tokens.map((token) => ({ value: token.id, label: token.name, description: token.owner, keywords: [token.id] })) : [])
	]);
	let modelOptions = $derived([{ value: '', label: t('limits-all-models') }, ...data.models.map((candidate) => ({ value: candidate, label: candidate }))]);
	$effect(() => {
		subjectType = editing?.subject_type ?? 'global';
		subjectId = editing?.subject_id ?? '';
		model = editing?.model ?? '';
		dimension = editing?.dimension ?? 'requests';
		window = editing?.window ?? 'hour';
		value = editing ? String(editing.value) : '';
	});

	function changeSubjectType(event: Event) {
		subjectType = (event.currentTarget as HTMLSelectElement).value as LimitSubjectType;
		subjectId = '';
	}

	async function submit() {
		saving = true;
		try {
			const subject = subjectType === 'global' ? t('limits-subject-global') : limitSubjectName({ subject_type: subjectType, subject_id: subjectId }, data);
			await onsave({ ...(editing ? { id: editing.id } : {}), subject_type: subjectType, subject_id: subjectId, model, dimension, window, value: Number(value) }, subject);
		} finally { saving = false; }
	}
</script>

<article class="card border border-base-300 bg-base-100">
	<div class="card-body gap-3">
		<h2 class="card-title text-base">{t(editing ? 'limits-edit-heading' : 'limits-add-heading')}</h2>
		<form class="m-0 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3" onsubmit={(event) => { event.preventDefault(); void submit(); }}>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-subject-type">{t('limits-field-subject')}</label><select id="limit-subject-type" class="select select-bordered select-sm w-full" value={subjectType} onchange={changeSubjectType}><option value="global">{t('limits-subject-global')}</option><option value="role">{t('limits-subject-role')}</option><option value="user">{t('limits-subject-user')}</option><option value="token">{t('limits-subject-token')}</option></select></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-subject-id">{t('limits-field-subject-id')}</label><SearchableSelect id="limit-subject-id" options={subjectOptions} bind:value={subjectId} ariaLabel={t('limits-field-subject-id')} size="sm" class="w-full" disabled={subjectType === 'global'} /></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-model">{t('limits-field-model')}</label><SearchableSelect id="limit-model" options={modelOptions} bind:value={model} ariaLabel={t('limits-field-model')} size="sm" class="w-full" /></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-dimension">{t('limits-field-dimension')}</label><select id="limit-dimension" class="select select-bordered select-sm w-full" bind:value={dimension}><option value="requests">{t('limits-dim-requests')}</option><option value="tokens">{t('limits-dim-tokens')}</option><option value="cost">{t('limits-dim-cost', { cur: data.currency })}</option></select></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-window">{t('limits-field-window')}</label><select id="limit-window" class="select select-bordered select-sm w-full" bind:value={window}><option value="hour">{t('limits-win-hour')}</option><option value="day">{t('limits-win-day')}</option><option value="week">{t('limits-win-week')}</option><option value="month">{t('limits-win-month')}</option></select></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-value">{t('limits-field-value')}</label><input id="limit-value" class="input input-bordered input-sm w-full" type="number" min="0" step="any" required bind:value={value} /></div>
			<div class="flex items-end"><button class="btn btn-primary btn-sm" type="submit" disabled={saving || !value || (subjectType !== 'global' && !subjectId)}>{t(editing ? 'limits-edit-submit' : 'limits-add-submit')}</button></div>
		</form>
	</div>
</article>
