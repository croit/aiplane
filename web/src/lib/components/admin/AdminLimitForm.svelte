<script lang="ts">
	import type { AdminLimitsData, LimitDimension, LimitSubjectType, LimitWindow } from '$lib/admin-limits';
	import { limitSubjectName } from '$lib/admin-limits';
	import { t } from '$lib/i18n.svelte';

	let { data, onsave }: { data: AdminLimitsData; onsave: (body: Record<string, string | number>, subject: string) => Promise<void> } = $props();
	let subjectType = $state<LimitSubjectType>('global');
	let subjectId = $state('');
	let model = $state('');
	let dimension = $state<LimitDimension>('requests');
	let window = $state<LimitWindow>('hour');
	let value = $state('');
	let saving = $state(false);

	function changeSubjectType(event: Event) {
		subjectType = (event.currentTarget as HTMLSelectElement).value as LimitSubjectType;
		subjectId = '';
	}

	async function submit() {
		saving = true;
		try {
			const subject = subjectType === 'global' ? t('limits-subject-global') : limitSubjectName({ subject_type: subjectType, subject_id: subjectId }, data);
			await onsave({ subject_type: subjectType, subject_id: subjectId, model, dimension, window, value: Number(value) }, subject);
		} finally { saving = false; }
	}
</script>

<article class="card border border-base-300 bg-base-100">
	<div class="card-body gap-3">
		<h2 class="card-title text-base">{t('limits-add-heading')}</h2>
		<form class="m-0 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3" onsubmit={(event) => { event.preventDefault(); void submit(); }}>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-subject-type">{t('limits-field-subject')}</label><select id="limit-subject-type" class="select select-bordered select-sm w-full" value={subjectType} onchange={changeSubjectType}><option value="global">{t('limits-subject-global')}</option><option value="role">{t('limits-subject-role')}</option><option value="user">{t('limits-subject-user')}</option><option value="token">{t('limits-subject-token')}</option></select></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-subject-id">{t('limits-field-subject-id')}</label><select id="limit-subject-id" class="select select-bordered select-sm w-full" bind:value={subjectId} disabled={subjectType === 'global'} required={subjectType !== 'global'}><option value="">{t('limits-field-subject-id-ph')}</option>{#if subjectType === 'role'}{#each data.roles as role (role)}<option value={role}>{role}</option>{/each}{:else if subjectType === 'user'}{#each data.users as user (user.id)}<option value={user.id}>{user.email}</option>{/each}{:else if subjectType === 'token'}{#each data.tokens as token (token.id)}<option value={token.id}>{token.name} ({token.owner})</option>{/each}{/if}</select></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-model">{t('limits-field-model')}</label><select id="limit-model" class="select select-bordered select-sm w-full" bind:value={model}><option value="">{t('limits-all-models')}</option>{#each data.models as candidate (candidate)}<option value={candidate}>{candidate}</option>{/each}</select></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-dimension">{t('limits-field-dimension')}</label><select id="limit-dimension" class="select select-bordered select-sm w-full" bind:value={dimension}><option value="requests">{t('limits-dim-requests')}</option><option value="tokens">{t('limits-dim-tokens')}</option><option value="cost">{t('limits-dim-cost', { cur: data.currency })}</option></select></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-window">{t('limits-field-window')}</label><select id="limit-window" class="select select-bordered select-sm w-full" bind:value={window}><option value="hour">{t('limits-win-hour')}</option><option value="day">{t('limits-win-day')}</option><option value="week">{t('limits-win-week')}</option><option value="month">{t('limits-win-month')}</option></select></div>
			<div class="flex flex-col gap-1"><label class="label-text text-xs opacity-70" for="limit-value">{t('limits-field-value')}</label><input id="limit-value" class="input input-bordered input-sm w-full" type="number" min="0" step="any" required bind:value={value} /></div>
			<div class="flex items-end"><button class="btn btn-primary btn-sm" type="submit" disabled={saving || !value || (subjectType !== 'global' && !subjectId)}>{t('limits-add-submit')}</button></div>
		</form>
	</div>
</article>
