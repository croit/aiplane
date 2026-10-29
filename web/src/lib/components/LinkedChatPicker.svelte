<script lang="ts">
	import { api } from '#lib/api.js';
	import { t } from '#lib/i18n.svelte.js';
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import { linkedChatOptions } from '#lib/linked-chat.js';
	import type { ChatSession } from '#lib/chat-protocol.js';

	/**
	 * Which chat a reusing schedule or webhook continues in. Shared by both
	 * forms, so the choice reads and behaves the same on each.
	 */
	let { value = $bindable(''), agentId = undefined }: { value?: string; agentId?: string | null } = $props();
	let sessions = $state<ChatSession[]>([]);
	let options = $derived(linkedChatOptions(sessions, {
		fresh: t('linked-chat-fresh'),
		untitled: t('nav-untitled-chat')
	}));

	$effect(() => {
		const scope = agentId;
		let cancelled = false;
		sessions = [];
		void api.listChatSessions(scope).then((result) => {
			if (!cancelled) sessions = result.sessions;
		}).catch(() => {
			if (!cancelled) sessions = [];
		});
		return () => { cancelled = true; };
	});
</script>

<fieldset class="fieldset w-full min-w-0">
	<legend class="fieldset-legend">{t('linked-chat-label')}</legend>
	<SearchableSelect {options} bind:value ariaLabel={t('linked-chat-label')} class="w-full" />
	<p class="label whitespace-normal">{t('linked-chat-help')}</p>
</fieldset>
