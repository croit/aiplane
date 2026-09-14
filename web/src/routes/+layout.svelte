<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { base } from '$app/paths';
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import { loginPageUrl } from '$lib/auth';
	import { navItemActive } from '$lib/nav';
	import { featureEnabled, featureForRoute, visibleNavLinks } from '$lib/features';
	import { pageTitleDescriptor } from '$lib/page-titles';
	import { pageTitleOverride } from '$lib/page-title';
	import { loadMe, me } from '$lib/session.svelte';
	import { sidebar, refreshSidebar, searchAsYouType, openSearch, closeSearch } from '$lib/sidebar.svelte';
	import { feedback, loadConfig } from '$lib/feedback.svelte';
	import { t, locale } from '$lib/i18n.svelte';
	import NavIcon, { type NavIconName } from '$lib/components/NavIcon.svelte';
	import LanguagePicker from '$lib/components/LanguagePicker.svelte';
	import SourceLink from '$lib/components/SourceLink.svelte';
	import ConversationSidebarRow from '$lib/components/chat/ConversationSidebarRow.svelte';
	import FeedbackFab from '$lib/components/feedback/FeedbackFab.svelte';
	import FeedbackDialog from '$lib/components/feedback/FeedbackDialog.svelte';

	let { children } = $props<{ children: import('svelte').Snippet }>();

	loadMe();

	// Signed out (401 from /api/v0/me) anywhere in the SPA → start the OIDC
	// login, coming back to the route the user actually wanted. The setup
	// wizard runs before any account exists — never bounce it.
	$effect(() => {
		const isSetup = page.url.pathname.startsWith(`${base}/setup`);
		if (me.loaded && me.value === null && !page.url.pathname.endsWith('/login') && !isSetup) {
			window.location.href = `${base}${loginPageUrl(page.url.pathname + page.url.search)}`;
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
			await refreshSidebar();
			await goto(`${base}/chat/${session.id}`);
		} catch {
			/* the layout's redirect handles 401 */
		}
	}

	async function pinChat(id: string, pinned: boolean) {
		try {
			await api.pinChatSession(id, pinned);
			await refreshSidebar();
		} catch (caught) {
			sidebarError = String(caught);
		}
	}

	async function removeChat(id: string) {
		try {
			await api.deleteChatSession(id);
			await refreshSidebar();
			if (page.url.pathname === `${base}/chat/${id}` || page.url.pathname === `/chat/${id}`) {
				await goto(`${base}/chat`);
			}
		} catch (caught) {
			sidebarError = String(caught);
		}
	}

	// ---- collapsible nav groups (same three as the legacy shell) ----------
	/// Surfaced only when sign-out fails; a silent failure would leave the
	/// session alive behind a signed-out-looking shell.
	let signOutError = $state<string | null>(null);
	let sidebarError = $state<string | null>(null);
	function savedNavSections(): Set<string> {
		if (typeof document === 'undefined') return new Set(['workspace']);
		const value = document.cookie
			.split('; ')
			.find((cookie) => cookie.startsWith('nav_sections='))
			?.slice('nav_sections='.length);
		return value === undefined ? new Set(['workspace']) : new Set(value.split(','));
	}

	const initialNavSections = savedNavSections();
	if (page.url.pathname.startsWith(`${base}/admin/`) || page.url.pathname.startsWith(`${base}/rag`)) {
		initialNavSections.add('admin');
	}
	let workspaceOpen = $state(initialNavSections.has('workspace'));
	let accountOpen = $state(initialNavSections.has('account'));
	let adminOpen = $state(initialNavSections.has('admin'));

	function toggleNavSection(name: string) {
		if (name === 'workspace') workspaceOpen = !workspaceOpen;
		else if (name === 'account') accountOpen = !accountOpen;
		else adminOpen = !adminOpen;
		const open = [
			workspaceOpen && 'workspace',
			accountOpen && 'account',
			adminOpen && 'admin'
		].filter(Boolean);
		document.cookie = `nav_sections=${open.length ? open.join(',') : 'none'}; path=/; max-age=31536000; samesite=lax`;
	}

	// Keys, not labels: the nav re-renders on a language switch because `t()`
	// reads the reactive locale, which only works if the lookup happens in the
	// template rather than once at module scope.
	type NavLink = [string, string, NavIconName];
	const workspaceLinks: NavLink[] = [
		['nav-memory', '/memory', 'folder'],
		['nav-scheduled', '/scheduled', 'clock'],
		['nav-webhooks', '/webhooks', 'send'],
		['nav-integrations', '/integrations', 'plug'],
		['nav-my-skills', '/skills', 'sparkles'],
		['nav-tools', '/tools', 'sliders']
	];
	const accountLinks: NavLink[] = [
		['nav-tokens', '/tokens', 'key'],
		['nav-usage', '/usage', 'chart']
	];
	const adminLinks: NavLink[] = [
		['nav-users', '/admin/users', 'users'],
		['nav-admin-tokens', '/admin/tokens', 'key'],
		['nav-groups', '/admin/groups', 'users'],
		['nav-upstreams', '/admin/upstreams', 'cube'],
		['nav-models', '/admin/models', 'cpu'],
		['nav-rag', '/rag', 'database'],
		['nav-skills', '/admin/skills', 'sparkles'],
		['nav-connectors', '/admin/connectors', 'plug'],
		['nav-comfyui', '/admin/comfyui', 'sparkles'],
		['nav-limits', '/admin/limits', 'sliders'],
		['nav-settings', '/admin/settings', 'sliders']
	];

	// Optional features the operator has switched off at /admin/settings take
	// their nav entries with them: an entry that leads to "this is not enabled"
	// is worse than no entry. The same map answers a URL typed by hand.
	const features = $derived(me.value?.features);
	const visibleWorkspaceLinks = $derived(visibleNavLinks(workspaceLinks, features));
	const visibleAccountLinks = $derived(visibleNavLinks(accountLinks, features));
	const visibleAdminLinks = $derived(visibleNavLinks(adminLinks, features));
	const routeFeature = $derived(featureForRoute(page.url.pathname, base));
	const featureOff = $derived(
		me.loaded && me.value !== null && routeFeature !== null && !featureEnabled(features, routeFeature)
	);

	const isAdmin = $derived(me.value?.role_ids?.includes('admin') ?? false);
	const publicRoute = $derived(page.url.pathname.startsWith(`${base}/setup`) || page.url.pathname.endsWith('/login'));
	const pageTitle = $derived(pageTitleDescriptor(page.url.pathname));
	const resolvedPageTitle = $derived(
		$pageTitleOverride.pathname === page.url.pathname && $pageTitleOverride.title
			? $pageTitleOverride.title
			: pageTitle
				? pageTitle.branded
					? t('page-title-branded', { title: t(pageTitle.key) })
					: t(pageTitle.key)
				: null
	);

	function isActive(path: string): boolean {
		return navItemActive(page.url.pathname, base, path);
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

<svelte:head>
	{#if resolvedPageTitle}
		<title>{resolvedPageTitle}</title>
	{/if}
</svelte:head>

{#if publicRoute}
	<div class="relative min-h-dvh bg-base-100 text-base-content"><div class="fixed right-4 top-4 z-20"><LanguagePicker placement="down" /></div><main class="flex min-h-dvh items-center justify-center p-6"><div class="w-full">{@render children()}</div></main></div>
{:else}
<div class="flex h-dvh overflow-hidden bg-base-100 text-base-content">
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
		class="fixed lg:sticky top-0 z-40 h-dvh w-72 shrink-0 flex flex-col bg-base-200 border-r border-base-300/60
			transition-transform -translate-x-full lg:translate-x-0 {sidebar.open ? 'translate-x-0' : ''}"
		aria-label={t('nav-main-aria')}
	>
		<!-- Brand -->
		<div class="px-4 pt-4 pb-2 flex items-center">
			<a href="{base}/chat" class="font-semibold">{t('nav-brand')}</a>
		</div>

		<!-- Primary nav -->
		<nav class="flex flex-col gap-0.5 px-2 pt-1 pb-2">
			<a
				href="{base}/chat"
				class="flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-sm {isChatActive()
					? 'bg-base-300 font-medium'
					: 'hover:bg-base-300/50'}"
				onclick={() => (sidebar.open = false)}
			>
				<NavIcon name="message" />
				{t('chat-default-title')}
			</a>

			{#snippet group(name: string, open: boolean, items: NavLink[])}
				{@const isOpen = name === 'workspace'
					? workspaceOpen
					: name === 'account'
						? accountOpen
						: adminOpen}
				<button
					class="w-full flex items-center gap-1 rounded-lg px-2.5 pt-2.5 pb-0.5 text-[11px] font-semibold tracking-wider text-base-content/50 uppercase hover:text-base-content/75"
					onclick={() => toggleNavSection(name)}
					aria-label={t('nav-group-toggle-aria', { label: t(`nav-group-${name}`) })}
					aria-expanded={isOpen}
				>
					{t(`nav-group-${name}`)}
					<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" class="ml-auto transition-transform {isOpen ? 'rotate-180' : ''}" aria-hidden="true"><polyline points="6 9 12 15 18 9"/></svg>
				</button>
				{#if isOpen}
					<div class="flex flex-col">
						{#each items as [label, path, icon] (path)}
							<a
								href="{base}{path}"
								class="flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-sm {isActive(path)
									? 'bg-base-300 font-medium'
									: 'hover:bg-base-300/50'}"
								onclick={() => (sidebar.open = false)}
							>
								<NavIcon name={icon} />
								{t(label)}
							</a>
						{/each}
					</div>
				{/if}
			{/snippet}

			{@render group('workspace', workspaceOpen, visibleWorkspaceLinks)}
			{@render group('account', accountOpen, visibleAccountLinks)}
			{#if isAdmin}
				{@render group('admin', adminOpen, visibleAdminLinks)}
			{/if}
		</nav>

		<!-- Conversations -->
		<div class="flex-1 min-h-0 flex flex-col mt-2 border-t border-base-300/60 px-2 pt-2">
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

			<ul class="flex-1 min-h-0 flex flex-col overflow-y-auto pb-2" data-sidebar-conversations>
				{#each sidebar.sessions as s (s.id)}
					<ConversationSidebarRow session={s} active={isActive(`/chat/${s.id}`)} onopen={() => (sidebar.open = false)} onpin={() => void pinChat(s.id, !s.pinned)} onremove={() => void removeChat(s.id)} />
				{:else}
					<li class="px-3 py-1.5 text-xs text-base-content/50">{t('chat-list-empty')}</li>
				{/each}
			</ul>
		</div>

		<!-- User footer -->
		<div class="border-t border-base-300/60 px-3 py-2 flex items-center gap-2">
			<span class="text-xs truncate flex-1 min-w-0" title={me.value?.email ?? ''}>
				{me.value?.email ?? ''}
			</span>
			<LanguagePicker />
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
		<div class="px-4 py-2 border-t border-base-300/60 text-[11px] leading-tight text-base-content/45">
			<SourceLink />
		</div>
	</aside>

	<!-- Main column -->
	<div class="flex h-dvh min-w-0 flex-1 flex-col">
		<!-- Mobile top bar with the menu toggle -->
		<div class="lg:hidden sticky top-0 z-20 h-14 flex items-center gap-2 px-3 bg-base-200 border-b border-base-300">
			<button class="btn btn-ghost btn-sm" onclick={() => (sidebar.open = true)} aria-label={t('nav-open-menu')}>
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><line x1="4" y1="6" x2="20" y2="6"/><line x1="4" y1="12" x2="20" y2="12"/><line x1="4" y1="18" x2="20" y2="18"/></svg>
			</button>
			<span class="font-semibold">{t('nav-brand')}</span>
		</div>

		<main class="min-h-0 min-w-0 flex-1 {isChatActive() ? 'overflow-hidden' : 'overflow-y-auto'}">
			<div class="w-full {isChatActive() ? 'h-full px-4 py-3 sm:px-6' : 'px-4 pb-8 pt-6 sm:px-6'}">
				{#if featureOff && routeFeature}
					<!-- The URL still resolves — the feature behind it does not.
					     Say which one, so an operator knows which switch to flip. -->
					<div class="w-full max-w-2xl">
						<h1 class="text-2xl font-bold">{t(`settings-s-${routeFeature.replaceAll('.', '-')}`)}</h1>
						<div class="alert alert-warning mt-4">
							<span>{t('feature-disabled-body', { feature: t(`settings-s-${routeFeature.replaceAll('.', '-')}`) })}</span>
						</div>
						{#if isAdmin}
							<a class="btn btn-sm mt-4" href="{base}/admin/settings">{t('feature-disabled-settings-link')}</a>
						{/if}
					</div>
				{:else}
					{@render children()}
				{/if}
			</div>
		</main>
	</div>

	{#if signOutError}
		<div data-feedback-toast class="toast toast-end z-50">
			<div class="alert alert-error text-sm">
				<span>{t('nav-sign-out-failed', { error: signOutError })}</span>
				<button class="btn btn-ghost btn-xs" onclick={() => (signOutError = null)}>
					{t('feedback-close-aria')}
				</button>
			</div>
		</div>
	{/if}

	{#if sidebarError}
		<div data-feedback-toast class="toast toast-end z-50"><div class="alert alert-error text-sm"><span>{sidebarError}</span><button class="btn btn-ghost btn-xs" onclick={() => (sidebarError = null)}>{t('feedback-close-aria')}</button></div></div>
	{/if}

	{#if feedback.enabled && me.value}
		<!-- Siblings of <main>, so the dialog survives client-side navigation and
		     the screenshot is taken of whatever route is on screen.
		     On a conversation page the FAB would land on top of the composer's
		     send/stop button, so the composer carries the entry point instead
		     and only the dialog is mounted here. -->
		{#if !isChatActive()}
			<FeedbackFab />
		{/if}
		<FeedbackDialog />
	{/if}
</div>
{/if}
