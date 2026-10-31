<script lang="ts">
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import { shareSubjectOptions, type ShareSubjectMatches } from '#lib/agents.js';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * Pick one person or group from a server search (a few matches from two
	 * characters on, never the roster). Agents and conversations share it: each
	 * passes the `search` of its own `share-subjects` endpoint. The chosen id
	 * is `subject`, its label `picked`; the caller adds the access or the
	 * button. The surrounding flex row lays out the two fields.
	 */
	let {
		search,
		kind = $bindable('user'),
		subject = $bindable(''),
		picked = $bindable(null),
		onerror
	}: {
		search: (q: string) => Promise<ShareSubjectMatches>;
		kind?: 'user' | 'group';
		subject?: string;
		picked?: { value: string; label: string } | null;
		onerror: (err: unknown) => void;
	} = $props();

	let found = $state<ShareSubjectMatches | null>(null);
	let asked = 0;
	const options = $derived.by(() => {
		const matches = shareSubjectOptions(found, kind);
		return picked && !matches.some((o) => o.value === picked!.value) ? [picked, ...matches] : matches;
	});

	async function run(q: string) {
		const ticket = ++asked;
		try {
			const result = await search(q);
			if (ticket === asked) found = result;
		} catch (err) {
			if (ticket === asked) onerror(err);
		}
	}

	function setKind(next: 'user' | 'group') {
		kind = next;
		subject = '';
		picked = null;
		found = null;
	}
</script>

<label class="flex flex-col gap-1">
	<span class="label-text">{t('agents-share-subject-kind')}</span>
	<select class="select w-32" value={kind} onchange={(e) => setKind(e.currentTarget.value as 'user' | 'group')}>
		<option value="user">{t('agents-share-kind-user')}</option>
		<option value="group">{t('agents-share-kind-group')}</option>
	</select>
</label>
<div class="flex flex-col gap-1">
	<span class="label-text">{t(`agents-share-kind-${kind}`)}</span>
	<SearchableSelect {options} bind:value={subject} onsearch={(q) => void run(q)} onchange={(v) => (picked = options.find((o) => o.value === v) ?? null)} ariaLabel={t(`agents-share-kind-${kind}`)} placeholder={t('agents-pick')} class="w-64 max-w-full" />
</div>
{#if found?.truncated}<div class="alert alert-info basis-full" role="status"><span>{t('organization-share-search-truncated')}</span></div>{/if}
