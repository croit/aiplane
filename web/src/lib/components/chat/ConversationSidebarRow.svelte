<script lang="ts">
	import { base } from '$app/paths';
	import { t } from '$lib/i18n.svelte';
	import type { SidebarSession } from '$lib/sidebar.svelte';

	let { session, active, onopen, onpin, onremove }: {
		session: SidebarSession;
		active: boolean;
		onopen: () => void;
		onpin: () => void;
		onremove: () => void;
	} = $props();
</script>

<li class="group relative flex min-w-0 items-center">
	<a
		href="{base}/chat/{session.id}"
		class="block min-w-0 flex-1 truncate rounded-lg py-1.5 pl-3 pr-16 text-sm {active ? 'bg-base-300 font-medium' : 'hover:bg-base-300/50'}"
		onclick={onopen}
	>
		<span class="block truncate">{session.title?.trim() || t('nav-untitled-chat')}</span>
		{#if session.snippet}<span class="block truncate text-xs font-normal normal-case text-base-content/50">{@html session.snippet}</span>{/if}
	</a>
	<div class="absolute right-1 flex items-center">
		<button
			class="btn btn-ghost btn-xs btn-square opacity-100 sm:opacity-0 sm:group-hover:opacity-100 sm:focus:opacity-100 {session.pinned ? 'text-warning sm:opacity-100' : ''}"
			onclick={onpin}
			aria-label={t(session.pinned ? 'nav-unpin-conversation' : 'nav-pin-conversation')}
			title={t(session.pinned ? 'nav-unpin-conversation' : 'nav-pin-conversation')}
		>
			<svg viewBox="0 0 24 24" width="13" height="13" fill={session.pinned ? 'currentColor' : 'none'} stroke="currentColor" stroke-width="1.75" aria-hidden="true"><path d="m12 2 3.1 6.3 6.9 1-5 4.9 1.2 6.8-6.2-3.2L5.8 21 7 14.2 2 9.3l6.9-1z"/></svg>
		</button>
		<button class="btn btn-ghost btn-xs btn-square text-error opacity-100 sm:opacity-0 sm:group-hover:opacity-100 sm:focus:opacity-100" onclick={onremove} aria-label={t('nav-delete-conversation')} title={t('nav-delete-conversation')}>
			<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6M10 11v5M14 11v5"/></svg>
		</button>
	</div>
</li>
