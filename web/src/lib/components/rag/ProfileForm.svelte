<script lang="ts">
	import { untrack } from 'svelte';
	import { adminDelete, adminPost, adminPut } from '$lib/admin-client';
	import { t } from '$lib/i18n.svelte';
	import { profileFieldsJson, type RagProfile, type RagProfileField } from '$lib/rag';

	function exampleFields(): RagProfileField[] {
		return [
			{ key: 'counterparty', label: t('rag-profile-example-counterparty-label'), type: 'text', description: t('rag-profile-example-counterparty-description'), filterable: true },
			{ key: 'date', label: t('rag-profile-example-date-label'), type: 'date', description: t('rag-profile-example-date-description'), filterable: true, sortable: true },
			{ key: 'amount', label: t('rag-profile-example-amount-label'), type: 'number', description: t('rag-profile-example-amount-description'), sortable: true }
		];
	}

	let { profile = null, onsaved, oncancel } = $props<{
		profile?: RagProfile | null;
		onsaved: (message: string) => void | Promise<void>;
		oncancel?: () => void;
	}>();
	const initialProfile = untrack(() => profile);
	let name = $state(initialProfile?.name ?? '');
	let description = $state(initialProfile?.description ?? '');
	let prompt = $state(initialProfile?.prompt ?? '');
	let fields = $state(profileFieldsJson(initialProfile?.fields ?? exampleFields()));
	let error = $state<string | null>(null);
	let busy = $state(false);

	async function save() {
		busy = true;
		error = null;
		try {
			const body = { name: name.trim(), description: description.trim() || null, prompt: prompt.trim(), fields: JSON.parse(fields) };
			if (profile) {
				const result = await adminPut<{ reindex_required_by: string[] }>(`/api/v0/rag/profiles/${encodeURIComponent(profile.name)}`, body);
				await onsaved(result.reindex_required_by.length ? t('rag-profile-toast-saved-reindex', { name: body.name, collections: result.reindex_required_by.join(', ') }) : t('rag-profile-toast-saved', { name: body.name }));
			} else {
				await adminPost('/api/v0/rag/profiles', body);
				await onsaved(t('rag-profile-toast-created', { name: body.name }));
				name = '';
				description = '';
				prompt = '';
				fields = profileFieldsJson(exampleFields());
			}
		} catch (caught) {
			error = String(caught);
		} finally {
			busy = false;
		}
	}

	async function remove() {
		if (!profile || !confirm(t('rag-profile-delete-confirm', { name: profile.name }))) return;
		try {
			await adminDelete(`/api/v0/rag/profiles/${encodeURIComponent(profile.name)}`);
			await onsaved(t('rag-profile-toast-deleted'));
		} catch (caught) {
			error = String(caught);
		}
	}
</script>

<div class="card card-border min-w-0 bg-base-100">
	<div class="card-body min-w-0 gap-3">
		{#if !profile}<h2 class="card-title text-base">{t('rag-profile-create-heading')}</h2>{/if}
		{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
		<div class="grid min-w-0 grid-cols-1 gap-3 [&>.fieldset]:min-w-0 md:grid-cols-2">
			<fieldset class="fieldset"><legend class="fieldset-legend">{t('rag-profile-label-name')}</legend><input class="input w-full font-mono" bind:value={name} /></fieldset>
			<fieldset class="fieldset"><legend class="fieldset-legend">{t('rag-profile-label-description')}</legend><input class="input w-full" bind:value={description} /></fieldset>
		</div>
		<fieldset class="fieldset min-w-0"><legend class="fieldset-legend">{t('rag-profile-label-prompt')}</legend><textarea class="textarea min-h-24 min-w-0 max-w-full w-full" bind:value={prompt} placeholder={t('rag-profile-prompt-placeholder')}></textarea></fieldset>
		<fieldset class="fieldset min-w-0"><legend class="fieldset-legend">{t('rag-profile-label-fields')}</legend><textarea class="textarea min-h-80 min-w-0 max-w-full w-full font-mono text-xs" bind:value={fields}></textarea><p class="label max-w-full whitespace-normal">{t('rag-profile-fields-help')}</p></fieldset>
		{#if profile}<div class="alert alert-warning"><span>{t('rag-profile-edit-warning')}</span></div>{/if}
		<div class="card-actions justify-end">
			{#if profile && !profile.builtin}<button class="btn btn-outline btn-error btn-sm" onclick={remove}>{t('rag-profile-button-delete')}</button>{/if}
			{#if oncancel}<button class="btn btn-sm" onclick={oncancel}>{t('rag-button-cancel')}</button>{/if}
			<button class="btn btn-primary btn-sm" onclick={save} disabled={busy || !name.trim() || !prompt.trim()}>{profile ? t('rag-profile-button-save') : t('rag-profile-button-create')}</button>
		</div>
	</div>
</div>
