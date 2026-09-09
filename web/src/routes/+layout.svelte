<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { base } from '$app/paths';
	import { onMount } from 'svelte';
	import { api, loginUrl } from '$lib/api';
	import { loadMe, me } from '$lib/session.svelte';
	import { sidebar, refreshSidebar, searchAsYouType, openSearch, closeSearch } from '$lib/sidebar.svelte';
	import { feedback, loadConfig, openDialog, submit } from '$lib/feedback.svelte';
	import { t, locale, setLocale, LOCALES, LOCALE_NAMES } from '$lib/i18n.svelte';
	import type { Locale } from '$lib/i18n.svelte';

	let { children } = $props<{ children: import('svelte').Snippet }>();

	loadMe();

	// Signed out (401 from /api/v0/me) anywhere in the SPA → start the OIDC
	// login, coming back to the route the user actually wanted. The setup
	// wizard runs before any account exists — never bounce it.
	$effect(() => {
		const isSetup = page.url.pathname.startsWith(`${base}/setup`);
		if (me.loaded && me.value === null && !page.url.pathname.endsWith('/login') && !isSetup) {
			window.location.href = loginUrl(page.url.pathname + page.url.search);
		}
	});

	// Mirrors `document.documentElement.dataset.theme`, which the inline script
	// in app.html sets before first paint. Kept as state only so the toggle's
	// aria-label can name the theme it switches *to*.
	let dark = $state(false);
	onMount(() => {
		dark = document.documentElement.dataset.theme === 'dark';
	});

	function setTheme(theme: 'light' | 'dark') {
		dark = theme === 'dark';
		document.documentElement.dataset.theme = theme;
		// Persisted as a cookie, not localStorage, so the inline script in
		// app.html can read it before first paint — that is what stops a
		// flash of the wrong theme on a hard load.
		document.cookie = `theme=${theme}; path=/; max-age=31536000; samesite=lax`;
	}

	async function signOut() {
		try {
			await api.logout();
		} catch (err) {
			// Navigating away regardless would show a signed-out shell while
			// the cookie is still live on the server — the one failure mode
			// that must not be silent.
			signOutError = String(err);
			return;
		}
		window.location.href = `${base}/`;
	}

	async function newChat() {
		try {
			const { session } = await api.createChatSession();
			await goto(`${base}/chat/${session.id}`);
		} catch {
			/* the layout's redirect handles 401 */
		}
	}

	// ---- collapsible nav groups (same three as the legacy shell) ----------
	let langOpen = $state(false);
	/// Surfaced only when sign-out fails; a silent failure would leave the
	/// session alive behind a signed-out-looking shell.
	let signOutError = $state<string | null>(null);
	let workspaceOpen = $state(true);
	let accountOpen = $state(true);
	let adminOpen = $state(true);

	// Keys, not labels: the nav re-renders on a language switch because `t()`
	// reads the reactive locale, which only works if the lookup happens in the
	// template rather than once at module scope.
	const workspaceLinks: [string, string][] = [
		['nav-memory', '/memory'],
		['nav-scheduled', '/scheduled'],
		['nav-webhooks', '/webhooks'],
		['nav-integrations', '/integrations'],
		['nav-my-skills', '/skills'],
		['nav-tools', '/tools']
	];
	const accountLinks: [string, string][] = [
		['nav-tokens', '/tokens'],
		['nav-usage', '/usage']
	];
	const adminLinks: [string, string][] = [
		['nav-users', '/admin/users'],
		['nav-admin-tokens', '/admin/tokens'],
		['nav-groups', '/admin/groups'],
		['nav-upstreams', '/admin/upstreams'],
		['nav-models', '/admin/models'],
		['nav-rag', '/admin/rag'],
		['nav-skills', '/admin/skills'],
		['nav-connectors', '/admin/connectors'],
		['nav-comfyui', '/admin/comfyui'],
		['nav-limits', '/admin/limits'],
		['nav-settings', '/admin/settings']
	];

	const isAdmin = $derived(me.value?.role_ids?.includes('admin') ?? false);

	function isActive(path: string): boolean {
		return page.url.pathname === `${base}${path}` || page.url.pathname === path;
	}

	function isChatActive(): boolean {
		return page.url.pathname.startsWith(`${base}/chat`);
	}

	onMount(() => {
		// Keep <html lang> in step with the catalog: screen readers and the
		// browser's own spellchecker read it, and app.html can only guess.
		document.documentElement.lang = locale.current;
		void refreshSidebar();
		void loadConfig();
		if ('serviceWorker' in navigator) {
			void navigator.serviceWorker.register(`${base}/sw.js`);
		}
	});

</script>

<div class="min-h-dvh bg-base-100 text-base-content flex">
	<!-- Mobile backdrop -->
	{#if sidebar.open}
		<button
			class="fixed inset-0 z-30 bg-black/50 lg:hidden"
			aria-label={t('nav-close-menu')}
			onclick={() => (sidebar.open = false)}
		></button>
	{/if}

	<!-- Sidebar -->
	<aside
		class="fixed lg:sticky top-0 z-40 h-dvh w-72 shrink-0 flex flex-col bg-base-200 border-r border-base-300
			transition-transform -translate-x-full lg:translate-x-0 {sidebar.open ? 'translate-x-0' : ''}"
		aria-label={t('nav-main-aria')}
	>
		<!-- Brand -->
		<div class="px-4 h-14 flex items-center border-b border-base-300">
			<a href="{base}/chat" class="font-semibold">{me.value ? 'LLM Gateway' : 'LLM Gateway'}</a>
		</div>

		<!-- Primary nav -->
		<nav class="flex-1 overflow-y-auto px-2 py-2">
			<a
				href="{base}/chat"
				class="flex items-center gap-2 rounded-lg px-3 py-2 text-sm {isChatActive()
					? 'bg-base-300 font-medium'
					: 'hover:bg-base-300/50'}"
				onclick={() => (sidebar.open = false)}
			>
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8z"/></svg>
				Chat
			</a>

			{#snippet group(name: string, open: boolean, items: [string, string][])}
				{@const toggle = () => {
					if (name === 'workspace') workspaceOpen = !workspaceOpen;
					else if (name === 'account') accountOpen = !accountOpen;
					else adminOpen = !adminOpen;
				}}
				{@const isOpen = name === 'workspace'
					? workspaceOpen
					: name === 'account'
						? accountOpen
						: adminOpen}
				<button
					class="w-full flex items-center gap-1 rounded-lg px-3 pt-4 pb-1 text-[11px] font-semibold tracking-wider text-base-content/50 uppercase hover:bg-base-300/40"
					onclick={toggle}
					aria-label={t('nav-group-toggle-aria', { label: t(`nav-group-${name}`) })}
				>
					{t(`nav-group-${name}`)}
					<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" class="ml-auto transition-transform {isOpen ? 'rotate-180' : ''}" aria-hidden="true"><polyline points="6 9 12 15 18 9"/></svg>
				</button>
				{#if isOpen}
					<div class="flex flex-col">
						{#each items as [label, path] (path)}
							<a
								href="{base}{path}"
								class="flex items-center gap-2 rounded-lg px-3 py-1.5 text-sm {isActive(path)
									? 'bg-base-300 font-medium'
									: 'hover:bg-base-300/50'}"
								onclick={() => (sidebar.open = false)}
							>
								{t(label)}
							</a>
						{/each}
					</div>
				{/if}
			{/snippet}

			{@render group('workspace', workspaceOpen, workspaceLinks)}
			{@render group('account', accountOpen, accountLinks)}
			{#if isAdmin}
				{@render group('admin', adminOpen, adminLinks)}
			{/if}
		</nav>

		<!-- Conversations -->
		<div class="border-t border-base-300 px-2 py-2">
			<div class="flex items-center justify-between px-2 py-1">
				<span class="text-[11px] font-semibold tracking-wider text-base-content/50 uppercase">{t('nav-conversations-label')}</span>
				<div class="flex gap-1">
					{#if sidebar.searching}
						<button class="btn btn-ghost btn-xs" onclick={closeSearch} aria-label={t('nav-search-close-aria')}>✕</button>
					{:else}
						<button class="btn btn-ghost btn-xs" onclick={openSearch} aria-label={t('nav-search-aria')}>
							<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/></svg>
						</button>
					{/if}
					<button class="btn btn-ghost btn-xs" onclick={newChat} aria-label={t('nav-new-conversation-aria')} title={t('nav-new-conversation-title')}>
						<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
					</button>
				</div>
			</div>

			{#if sidebar.searching}
				<div class="px-2 pb-1">
					<input
						class="input input-sm input-bordered w-full"
						placeholder={t('nav-search-placeholder')}
						value={sidebar.query}
						oninput={(e) => searchAsYouType((e.currentTarget as HTMLInputElement).value)}
						aria-label={t('nav-search-aria')}
					/>
				</div>
			{/if}

			<ul class="flex flex-col max-h-64 overflow-y-auto">
				{#each sidebar.sessions as s (s.id)}
					<li>
						<a
							href="{base}/chat/{s.id}"
							class="block rounded-lg px-3 py-1.5 text-sm truncate {isActive(`/chat/${s.id}`)
								? 'bg-base-300 font-medium'
								: 'hover:bg-base-300/50'}"
							onclick={() => (sidebar.open = false)}
						>
							{s.title?.trim() || t('nav-untitled-chat')}
							{#if s.snippet}
								<span class="block text-xs text-base-content/50 truncate normal-case">{@html s.snippet}</span>
							{/if}
						</a>
					</li>
				{:else}
					<li class="px-3 py-1.5 text-xs text-base-content/50">No conversations.</li>
				{/each}
			</ul>
		</div>

		<div class="px-4 py-1 border-t border-base-300/60">
			<span class="text-[11px] text-base-content/45">Source · AGPL-3.0 · v0.1.0</span>
		</div>
		<!-- User footer -->
		<div class="border-t border-base-300 px-3 py-2 flex items-center gap-2">
			<span class="text-xs truncate flex-1 min-w-0" title={me.value?.email ?? ''}>
				{me.value?.email ?? ''}
			</span>
			<div class="relative">
				<button
					class="btn btn-ghost btn-xs"
					onclick={() => (langOpen = !langOpen)}
					aria-label={t('chrome-lang-switcher-aria')}
					title={t('chrome-lang-switcher-aria')}
				>
					<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><circle cx="12" cy="12" r="10"/><path d="M2 12h20"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>
				</button>
				{#if langOpen}
					<!-- Close on an outside click, so the menu never strands. -->
					<button
						class="fixed inset-0 z-40 cursor-default bg-transparent"
						aria-label={t('nav-close-menu')}
						onclick={() => (langOpen = false)}
					></button>
					<ul class="absolute bottom-9 right-0 z-50 menu bg-base-200 rounded-box border border-base-300 shadow p-1 w-36">
						{#each LOCALES as code (code)}
							<li>
								<button
									class="text-sm {locale.current === code ? 'font-semibold' : ''}"
									onclick={() => {
										// Fire-and-forget: `setLocale` awaits the catalog chunk
										// and only then moves the locale, so the app re-renders
										// in one step rather than half-translated.
										void setLocale(code);
										langOpen = false;
									}}
								>
									{LOCALE_NAMES[code]}
								</button>
							</li>
						{/each}
					</ul>
				{/if}
			</div>
			<button
				class="btn btn-ghost btn-xs"
				title={t('chrome-theme-toggle-title')}
				aria-label={dark ? t('chrome-theme-toggle-aria-to-light') : t('chrome-theme-toggle-aria-to-dark')}
				onclick={() => setTheme(dark ? 'light' : 'dark')}
			>
					<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/></svg>
			</button>
			<button
				class="btn btn-ghost btn-xs"
				title={t('nav-sign-out')}
				aria-label={t('nav-sign-out')}
				onclick={() => void signOut()}
			>
					<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/></svg>
			</button>
		</div>
	</aside>

	<!-- Main column -->
	<div class="flex-1 min-w-0 flex flex-col min-h-dvh">
		<!-- Mobile top bar with the menu toggle -->
		<div class="lg:hidden sticky top-0 z-20 h-14 flex items-center gap-2 px-3 bg-base-200 border-b border-base-300">
			<button class="btn btn-ghost btn-sm" onclick={() => (sidebar.open = true)} aria-label={t('nav-open-menu')}>
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><line x1="4" y1="6" x2="20" y2="6"/><line x1="4" y1="12" x2="20" y2="12"/><line x1="4" y1="18" x2="20" y2="18"/></svg>
			</button>
			<span class="font-semibold">LLM Gateway</span>
		</div>

		<main class="flex-1 min-h-0 min-w-0 overflow-y-auto">
			<div class="w-full max-w-5xl mx-auto px-4 sm:px-6 pt-6 pb-8">
				{@render children()}
			</div>
		</main>
	</div>

	{#if signOutError}
		<div class="toast toast-end z-50">
			<div class="alert alert-error text-sm">
				<span>{t('nav-sign-out-failed', { error: signOutError })}</span>
				<button class="btn btn-ghost btn-xs" onclick={() => (signOutError = null)}>
					{t('feedback-close-aria')}
				</button>
			</div>
		</div>
	{/if}

	{#if feedback.enabled && me.value}
		<button class="btn btn-circle btn-neutral fixed bottom-4 right-4 z-40" onclick={openDialog} aria-label={t('feedback-fab-aria')}>
			?
		</button>
	{/if}

	{#if feedback.open}
		<dialog class="modal modal-open" aria-label={t('feedback-dialog-heading')}>
			<div class="modal-box max-w-lg">
				{#if feedback.submitted}
					<h2 class="text-lg font-semibold mb-2">{t('feedback-thanks-heading')}</h2>
					<p class="text-sm text-base-content/70">{t('feedback-thanks-body')}</p>
					<div class="modal-action"><button class="btn btn-primary btn-sm" onclick={() => (feedback.open = false)}>{t('feedback-done-button')}</button></div>
				{:else}
					<h2 class="text-lg font-semibold mb-3">{t('feedback-dialog-heading')}</h2>
					<div class="flex flex-col gap-3">
						<input class="input input-bordered input-sm" placeholder={t('feedback-title-placeholder')} bind:value={feedback.title} />
						<textarea class="textarea textarea-bordered text-sm" rows="3" placeholder={t('feedback-description-placeholder')} bind:value={feedback.description}></textarea>
						<textarea class="textarea textarea-bordered text-sm" rows="2" placeholder={t('feedback-business-placeholder')} bind:value={feedback.business}></textarea>
						<textarea class="textarea textarea-bordered text-sm" rows="2" placeholder={t('feedback-acceptance-placeholder')} bind:value={feedback.acceptance}></textarea>
						<select class="select select-bordered select-sm" bind:value={feedback.priority}>
							<option value="low">{t('feedback-priority-low')}</option><option value="medium">{t('feedback-priority-medium')}</option><option value="high">{t('feedback-priority-high')}</option>
						</select>
						{#if feedback.error}<div class="alert alert-error py-2 text-sm"><span>{feedback.error}</span></div>{/if}
					</div>
					<div class="modal-action">
						<button class="btn btn-ghost btn-sm" onclick={() => (feedback.open = false)}>{t('feedback-cancel-button')}</button>
						<button class="btn btn-primary btn-sm" onclick={submit} disabled={feedback.busy || !feedback.title.trim() || !feedback.description.trim()}>
							{feedback.busy ? t('feedback-sending') : t('feedback-submit-button')}
						</button>
					</div>
				{/if}
			</div>
			<form method="dialog" class="modal-backdrop"><button onclick={() => (feedback.open = false)}>{t('feedback-close-aria')}</button></form>
		</dialog>
	{/if}
</div>
