<script lang="ts">
	import Modal from '#lib/components/ui/Modal.svelte';
	import SubjectPicker from '#lib/components/sharing/SubjectPicker.svelte';
	import { chatClassificationApi, failure, sharesBody, type ChatShares } from '#lib/classification.js';
	import { request } from '#lib/api.js';
	import type { ShareSubjectMatches } from '#lib/agents.js';
	import { copyOnceSaved } from '#lib/clipboard.js';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * Share one conversation either by link with every user of this
	 * installation, where its level allows, or with named people and groups.
	 * The picker is the agents' one; the server's refusal (a link above the
	 * highest shareable level) is shown as it is. Saving a link copies it.
	 */
	let { open = $bindable(false), sessionId, onsaved }: { open: boolean; sessionId: string; onsaved: (linkCopied: boolean) => void | Promise<void> } = $props();

	type Mode = 'none' | 'link' | 'people';

	let shares = $state<ChatShares | null>(null);
	let mode = $state<Mode>('none');
	let error = $state<string | null>(null);
	let busy = $state(false);
	let kind = $state<'user' | 'group'>('user');
	let subject = $state('');
	let picked = $state<{ value: string; label: string } | null>(null);

	$effect(() => {
		if (!open) return;
		shares = null;
		error = null;
		void chatClassificationApi.shares(sessionId).then(
			(s) => {
				shares = s;
				mode = s.link && s.link_allowed ? 'link' : s.subjects.length > 0 ? 'people' : 'none';
			},
			(err) => (error = failure(err))
		);
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
		const body = sharesBody(mode === 'link', mode === 'people' ? shares.subjects : []);
		const saved = chatClassificationApi.saveShares(sessionId, body).then(
			() => true,
			(err) => {
				error = failure(err);
				return false;
			}
		);
		const copied = mode === 'link' ? copyOnceSaved(window.location.href, saved) : Promise.resolve(false);
		try {
			if (!(await saved)) return;
			await onsaved(await copied);
			open = false;
		} finally {
			busy = false;
		}
	}
</script>

<Modal bind:open title={t('share-dialog-title')} size="md">
	{#if error}<div class="alert alert-error mb-3 text-sm" role="alert"><span>{error}</span></div>{/if}
	{#if shares}
		<div class="space-y-3">
			<label class="flex items-center gap-3">
				<input type="radio" class="radio radio-sm" name="share-mode" value="none" bind:group={mode} />
				<span class="text-sm">{t('share-dialog-mode-none')}</span>
			</label>
			<label class="flex items-start gap-3">
				<input type="radio" class="radio radio-sm mt-0.5" name="share-mode" value="link" bind:group={mode} disabled={!shares.link_allowed} />
				<span class="text-sm">
					{t('share-dialog-mode-link')}
					{#if !shares.link_allowed}<span class="block text-base-content/60">{t('share-dialog-link-not-allowed')}</span>{/if}
				</span>
			</label>
			<label class="flex items-start gap-3">
				<input type="radio" class="radio radio-sm mt-0.5" name="share-mode" value="people" bind:group={mode} />
				<span class="text-sm">
					{t('share-dialog-mode-people')}
					<span class="block text-base-content/60">{t('share-dialog-lead')}</span>
				</span>
			</label>
			{#if mode === 'people'}
				<div class="space-y-3 border-l-2 border-base-300 pl-4">
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
			{/if}
		</div>
	{:else if !error}
		<span class="loading loading-spinner"></span>
	{/if}
	{#snippet footer()}
		<button class="btn btn-ghost" type="button" onclick={() => (open = false)}>{t('admin-cancel')}</button>
		<button class="btn btn-primary" type="button" disabled={busy || !shares} onclick={save}>{t('share-dialog-save')}</button>
	{/snippet}
</Modal>
