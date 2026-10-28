<script lang="ts">
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import { agentsApi, ownerLabel, SHARE_ACCESS, shareSubjectLabel, shareSubjectOptions, type AgentError, type Share, type ShareAccess, type ShareSubjectMatches } from '#lib/agents.js';
	import { locale, t } from '#lib/i18n.svelte.js';

	/**
	 * Who else may work on this agent, and who answers its inbox. `read` and
	 * `write` take effect only for holders of the agent-management permission
	 * (they show the spec and the visitors' conversations); `respond` answers
	 * the inbox items and is open to every user and group. A subject is picked
	 * from a server search (`share-subjects`: a few matches from two
	 * characters on, never the roster), never typed in. Whether it may hold
	 * `read` or `write`, and that the last `write` share stays, the server says
	 * when it refuses; that is shown as it is.
	 */
	let { agentId, shares, writable, onchanged }: {
		agentId: string;
		shares: Share[];
		writable: boolean;
		onchanged: () => void | Promise<void>;
	} = $props();

	let kind = $state<Share['subject_kind']>('user');
	let access = $state<ShareAccess>('respond');
	let subject = $state('');
	let picked = $state<{ value: string; label: string } | null>(null);
	let found = $state<ShareSubjectMatches | null>(null);
	let error = $state<string | null>(null);
	let busy = $state(false);
	let asked = 0;
	const options = $derived.by(() => {
		const matches = shareSubjectOptions(found, kind);
		return picked && !matches.some((o) => o.value === picked!.value) ? [picked, ...matches] : matches;
	});

	async function search(q: string) {
		const ticket = ++asked;
		try {
			const result = await agentsApi.shareSubjects(agentId, q);
			if (ticket === asked) found = result;
		} catch (err) {
			if (ticket === asked) error = (err as AgentError).message;
		}
	}

	function pick(value: string) {
		picked = options.find((o) => o.value === value) ?? null;
	}

	function setKind(next: Share['subject_kind']) {
		kind = next;
		subject = '';
		picked = null;
		found = null;
	}

	async function run(action: () => Promise<unknown>) {
		busy = true;
		error = null;
		try {
			await action();
			await onchanged();
		} catch (err) {
			error = (err as AgentError).message;
		} finally {
			busy = false;
		}
	}
	const add = () =>
		run(async () => {
			await agentsApi.share(agentId, { subject_kind: kind, subject_id: subject, access });
			subject = '';
			picked = null;
		});
</script>

<div class="space-y-4">
	<p class="text-sm text-base-content/70">{t('agents-share-intro')}</p>
	{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}

	<div class="overflow-x-auto">
		<table class="table table-sm">
			<thead>
				<tr><th>{t('agents-share-subject')}</th><th>{t('agents-share-access')}</th><th></th></tr>
			</thead>
			<tbody>
				{#each shares as share (share.subject_kind + share.subject_id)}
					<tr>
						<td>
							<span class="badge badge-outline mr-2">{t(`agents-share-kind-${share.subject_kind}`)}</span>
							<span class="text-sm break-all">{shareSubjectLabel(share)}</span>
							{#if share.granted}<p class="text-xs text-base-content/60">{t('agents-shared-by', { name: ownerLabel(share.granted.by), date: new Date(share.granted.at).toLocaleDateString(locale.current) })}</p>{/if}
						</td>
						<td>
							{#if writable}
								<select class="select select-xs w-32" value={share.access} disabled={busy} onchange={(e) => run(() => agentsApi.share(agentId, { subject_kind: share.subject_kind, subject_id: share.subject_id, access: e.currentTarget.value as ShareAccess }))} aria-label={t('agents-share-access')}>
									{#each SHARE_ACCESS as level (level)}<option value={level}>{t(`agents-share-${level}`)}</option>{/each}
								</select>
							{:else}
								{t(`agents-share-${share.access}`)}
							{/if}
						</td>
						<td class="text-right">
							{#if writable}
								<button class="btn btn-ghost btn-xs" type="button" disabled={busy} onclick={() => run(() => agentsApi.revokeShare(agentId, share))}>{t('agents-share-revoke')}</button>
							{/if}
						</td>
					</tr>
				{:else}
					<tr><td colspan="3" class="text-sm text-base-content/60">{t('agents-share-empty')}</td></tr>
				{/each}
			</tbody>
		</table>
	</div>

	{#if found?.truncated}<div class="alert alert-info" role="status"><span>{t('organization-share-search-truncated')}</span></div>{/if}
	{#if writable}
		<form class="flex flex-wrap items-end gap-3" onsubmit={(e) => { e.preventDefault(); void add(); }}>
			<label class="flex flex-col gap-1">
				<span class="label-text">{t('agents-share-access')}</span>
				<select class="select w-32" bind:value={access}>
					{#each SHARE_ACCESS as level (level)}<option value={level}>{t(`agents-share-${level}`)}</option>{/each}
				</select>
			</label>
			<label class="flex flex-col gap-1">
				<span class="label-text">{t('agents-share-subject-kind')}</span>
				<select class="select w-32" value={kind} onchange={(e) => setKind(e.currentTarget.value as Share['subject_kind'])}>
					<option value="user">{t('agents-share-kind-user')}</option>
					<option value="group">{t('agents-share-kind-group')}</option>
				</select>
			</label>
			<div class="flex flex-col gap-1">
				<span class="label-text">{t(`agents-share-kind-${kind}`)}</span>
				<SearchableSelect {options} bind:value={subject} onsearch={(q) => void search(q)} onchange={pick} ariaLabel={t(`agents-share-kind-${kind}`)} placeholder={t('agents-pick')} class="w-64 max-w-full" />
			</div>
			<button class="btn btn-primary" type="submit" disabled={busy || !subject}>{t('agents-share-add')}</button>
		</form>
	{/if}
</div>
