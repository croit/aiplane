<script lang="ts">
	import Modal from '#lib/components/ui/Modal.svelte';
	import SubjectPicker from '#lib/components/sharing/SubjectPicker.svelte';
	import { chatClassificationApi, failure, sharesBody, type ChatShares } from '#lib/classification.js';
	import { request } from '#lib/api.js';
	import type { ShareSubjectMatches } from '#lib/agents.js';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * Share one conversation with people and groups, and, where its level
	 * allows, with anyone who has the link. The picker is the agents' one; the
	 * server's refusal (a link above the highest shareable level) is shown as
	 * it is.
	 */
	let { open = $bindable(false), sessionId, onsaved }: { open: boolean; sessionId: string; onsaved: () => void | Promise<void> } = $props();

	let shares = $state<ChatShares | null>(null);
	let error = $state<string | null>(null);
	let busy = $state(false);
	let kind = $state<'user' | 'group'>('user');
	let subject = $state('');
	let picked = $state<{ value: string; label: string } | null>(null);

	$effect(() => {
		if (!open) return;
		shares = null;
		error = null;
		void chatClassificationApi.shares(sessionId).then((s) => (shares = s), (err) => (error = failure(err)));
	});

	const search = (q: string) =>
		request<ShareSubjectMatches>(`/api/v0/chat/sessions/${encodeURIComponent(sessionId)}/share-subjects?q=${encodeURIComponent(q)}`);

	function add() {
		if (!shares || !picked || shares.subjects.some((s) => s.kind === kind && s.id === subject)) return;
		shares.subjects = [...shares.subjects, { kind, id: subject, name: picked.label }];
		subject = '';
		picked = null;
	}

	async function save() {
		if (!shares) return;
		busy = true;
		error = null;
		try {
			await chatClassificationApi.saveShares(sessionId, sharesBody(shares.link_allowed && shares.link, shares.subjects));
			await onsaved();
			open = false;
		} catch (err) {
			error = failure(err);
		} finally {
			busy = false;
		}
	}
</script>

<Modal bind:open title={t('share-dialog-title')} description={t('share-dialog-lead')} size="md">
	{#if error}<div class="alert alert-error mb-3 text-sm" role="alert"><span>{error}</span></div>{/if}
	{#if shares}
		<div class="space-y-4">
			{#if shares.link_allowed}
				<label class="flex items-center gap-3">
					<input type="checkbox" class="toggle toggle-sm" bind:checked={shares.link} />
					<span class="text-sm">{t('share-dialog-link')}</span>
				</label>
			{:else}
				<div class="alert alert-info text-sm" role="status"><span>{t('share-dialog-link-not-allowed')}</span></div>
			{/if}
			<ul class="m-0 list-none space-y-1 p-0">
				{#each shares.subjects as s (s.kind + s.id)}
					<li class="flex items-center gap-2">
						<span class="badge badge-outline">{t(`agents-share-kind-${s.kind}`)}</span>
						<span class="min-w-0 flex-1 break-all text-sm">{s.name || s.id}</span>
						<button class="btn btn-ghost btn-xs" type="button" onclick={() => (shares!.subjects = shares!.subjects.filter((x) => x !== s))}>{t('agents-share-revoke')}</button>
					</li>
				{:else}
					<li class="text-sm text-base-content/60">{t('share-dialog-empty')}</li>
				{/each}
			</ul>
			<form class="flex flex-wrap items-end gap-3" onsubmit={(e) => { e.preventDefault(); add(); }}>
				<SubjectPicker {search} bind:kind bind:subject bind:picked onerror={(err) => (error = failure(err))} />
				<button class="btn btn-sm" type="submit" disabled={!subject}>{t('agents-share-add')}</button>
			</form>
		</div>
	{:else if !error}
		<span class="loading loading-spinner"></span>
	{/if}
	{#snippet footer()}
		<button class="btn btn-ghost" type="button" onclick={() => (open = false)}>{t('admin-cancel')}</button>
		<button class="btn btn-primary" type="button" disabled={busy || !shares} onclick={save}>{t('share-dialog-save')}</button>
	{/snippet}
</Modal>
