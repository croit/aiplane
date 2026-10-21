<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { api } from '#lib/api.js';
	import { organizationsApi } from '#lib/organizations.js';
	import { loginPageUrl } from '#lib/auth.js';
	import { navItemActive } from '#lib/nav.js';
	import { boundedViewport } from '#lib/viewport.js';
	import { featureEnabled, featureForRoute, visibleNavLinks } from '#lib/features.js';
	import { pageTitleDescriptor } from '#lib/page-titles.js';
	import { pageTitleOverride } from '#lib/page-title.js';
	import { loadMe, me } from '#lib/session.svelte.js';
	import { inboxLive, watchInbox } from '#lib/inbox.svelte.js';
	import { inboxShown } from '#lib/inbox.js';
	import { sidebar, refreshSidebar, searchAsYouType, openSearch, closeSearch } from '#lib/sidebar.svelte.js';
	import { feedback, loadConfig } from '#lib/feedback.svelte.js';
	import { t, locale } from '#lib/i18n.svelte.js';
	import NavIcon, { type NavIconName } from '#lib/components/NavIcon.svelte';
	import BrandLockup from '#lib/components/BrandLockup.svelte';
	import LanguagePicker from '#lib/components/LanguagePicker.svelte';
	import SourceLink from '#lib/components/SourceLink.svelte';
	import ConversationSidebarRow from '#lib/components/chat/ConversationSidebarRow.svelte';
	import FeedbackFab from '#lib/components/feedback/FeedbackFab.svelte';
	import FeedbackDialog from '#lib/components/feedback/FeedbackDialog.svelte';

	let { children } = $props<{ children: import('svelte').Snippet }>();

	loadMe();

	// Signed out (401 from /api/v0/me) anywhere in the SPA → start the OIDC
	// login, coming back to the route the user actually wanted. The setup
	// wizard runs before any account exists — never bounce it.
	$effect(() => {
		const isSetup = page.url.pathname.startsWith('/setup');
		if (me.loaded && me.value === null && !page.url.pathname.endsWith('/login') && !isSetup) {
			window.location.href = loginPageUrl(page.url.pathname + page.url.search);
		}
	});

	// Anyone may be asked to answer: a responder of an agent, or the owner of
	// a scheduled run that paused. So the badge's stream opens for every
	// signed-in user.
	$effect(() => {
		if (me.value) watchInbox();
	});

	// Mirrors `document.documentElement.dataset.theme`, which the inline script
	// in app.html sets before first paint. Kept as state only so the toggle's
	// aria-label can name the theme it switches *to*.
	let dark = $state(true);
	onMount(() => {
		dark = document.documentElement.dataset.theme === 'dark';
	});

	function setTheme(theme: 'light' | 'dark') {
		dark = theme === 'dark';
		document.documentElement.dataset.theme = theme;
		document.querySelector('meta[name="theme-color"]')?.setAttribute('content', theme === 'dark' ? '#1d1d1b' : '#ffffff');
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
		window.location.href = '/';
	}

	// A second press while the first is in flight would race it past the
	// server's blank-conversation lookup and mint a duplicate.
	let openingNewChat = false;

	async function newChat() {
		if (openingNewChat) return;
		openingNewChat = true;
		try {
			const { session } = await api.createChatSession();
			await refreshSidebar();
			await goto(`/chat/${session.id}`);
		} catch (caught) {
			sidebarError = String(caught);
		} finally {
			openingNewChat = false;
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
			if (page.url.pathname === `/chat/${id}`) {
				await goto('/chat');
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
	let organizationsAvailable = $state(false);
	function savedNavSections(): Set<string> {
		if (typeof document === 'undefined') return new Set(['workspace']);
		const value = document.cookie
			.split('; ')
			.find((cookie) => cookie.startsWith('nav_sections='))
			?.slice('nav_sections='.length);
		return value === undefined ? new Set(['workspace']) : new Set(value.split(','));
	}

	const initialNavSections = savedNavSections();
	if (page.url.pathname.startsWith('/admin/') || page.url.pathname.startsWith('/rag')) {
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
		['nav-inbox', '/inbox', 'inbox'],
		['nav-scheduled', '/scheduled', 'clock'],
		['nav-webhooks', '/webhooks', 'send'],
		['nav-agents', '/agents', 'cpu'],
		['nav-dots', '/dots', 'cube'],
		['organizations-heading', '/organizations', 'users'],
		['nav-tools', '/tools', 'sliders']
	];
	const accountLinks: NavLink[] = [
		['user-settings-heading', '/settings', 'users'],
		['nav-usage', '/usage', 'chart']
	];
	const adminLinks: NavLink[] = [
		['admin-access-heading', '/admin/users', 'users'],
		['nav-models', '/admin/models', 'cpu'],
		['nav-rag', '/rag', 'database'],
		['nav-skills', '/admin/skills', 'sparkles'],
		['nav-connectors', '/admin/connectors', 'plug'],
		['nav-comfyui', '/admin/comfyui', 'sparkles'],
		['nav-settings', '/admin/settings', 'sliders']
	];

	// Optional features the operator has switched off at /admin/settings take
	// their nav entries with them: an entry that leads to "this is not enabled"
	// is worse than no entry. The same map answers a URL typed by hand.
	const features = $derived(me.value?.features);
	const visibleWorkspaceLinks = $derived(
		visibleNavLinks(workspaceLinks, features).filter(
			([, path]) =>
				(path !== '/agents' || me.value?.can_manage_agents) &&
				(path !== '/dots' || me.value?.can_manage_dots) &&
				(path !== '/organizations' || organizationsAvailable) &&
				(path !== '/inbox' || inboxShown(inboxLive, isActive('/inbox')))
		)
	);
	const visibleAccountLinks = $derived(visibleNavLinks(accountLinks, features));
	const visibleAdminLinks = $derived(visibleNavLinks(adminLinks, features));
	const routeFeature = $derived(featureForRoute(page.url.pathname));
	const featureOff = $derived(
		me.loaded && me.value !== null && routeFeature !== null && !featureEnabled(features, routeFeature)
	);

	const isAdmin = $derived(me.value?.role_ids?.includes('admin') ?? false);
	const publicRoute = $derived(page.url.pathname.startsWith('/setup') || page.url.pathname.endsWith('/login'));
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
		return navItemActive(page.url.pathname, path);
	}

	function isChatActive(): boolean {
		return page.url.pathname.startsWith('/chat');
	}

	const bounded = $derived(boundedViewport(page.url));

	$effect(() => {
		if (!me.loaded || !me.value) return;
		void organizationsApi.list()
			.then(({ organizations }) => {
				organizationsAvailable = organizations.length > 0;
			})
			.catch((error) => {
				console.error('loading /api/v0/organizations failed', error);
				organizationsAvailable = false;
			});
	});

	onMount(() => {
		// Keep <html lang> in step with the catalog: screen readers and the
		// browser's own spellchecker read it, and app.html can only guess.
		document.documentElement.lang = locale.current;
		void refreshSidebar();
		void loadConfig();
		if ('serviceWorker' in navigator) {
			void navigator.serviceWorker.register('/sw.js');
		}
	});

</script>

<svelte:head>
	{#if resolvedPageTitle}
		<title>{resolvedPageTitle}</title>
	{/if}
</svelte:head>

{#if publicRoute}
	<div class="relative isolate min-h-dvh overflow-hidden bg-base-100 text-base-content">
		<div class="pointer-events-none absolute -left-20 -top-24 h-72 w-72 rounded-full bg-secondary/15 blur-3xl"></div>
		<div class="pointer-events-none absolute -right-20 top-16 h-80 w-80 rounded-full bg-primary/20 blur-3xl"></div>
		<div class="absolute left-5 top-5 z-20 sm:left-8 sm:top-8"><a href="/chat" aria-label={t('nav-brand')}><BrandLockup /></a></div>
		<div class="absolute right-4 top-4 z-20"><LanguagePicker placement="down" /></div>
		<main class="relative flex min-h-dvh items-center justify-center px-6 pb-8 pt-24"><div class="w-full">{@render children()}</div></main>
	</div>
{:else}
<div class="flex h-dvh overflow-clip bg-base-100 text-base-content">
	<!-- Mobile backdrop -->
	{#if sidebar.open}
		<button
			class="fixed inset-0 z-30 bg-neutral/50 lg:hidden"
			aria-label={t('nav-close-menu')}
			onclick={() => (sidebar.open = false)}
		></button>
	{/if}

	<!-- Sidebar -->
	<aside
		class="fixed lg:sticky top-0 z-40 h-dvh w-72 shrink-0 flex flex-col bg-base-200/85 backdrop-blur-xl border-r border-base-300/60
			transition-transform -translate-x-full lg:translate-x-0 {sidebar.open ? 'translate-x-0' : ''}"
		aria-label={t('nav-main-aria')}
	>
		<div class="h-1 shrink-0 bg-linear-to-r from-secondary to-primary"></div>
		<!-- Brand -->
		<div class="px-4 pt-5 pb-5 flex items-center">
			<a href="/chat" aria-label={t('nav-brand')}><BrandLockup /></a>
		</div>

		<!-- Primary nav -->
		<nav class="flex flex-col gap-0.5 px-2 pt-1 pb-2">
			<button
				class="flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-sm text-left hover:bg-primary/10"
				onclick={() => {
					sidebar.open = false;
					void newChat();
				}}
			>
				<NavIcon name="message" />
				{t('nav-new-chat')}
			</button>

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
								href={path}
								class="flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-sm {isActive(path)
									? 'bg-primary/15 text-primary dark:text-base-content font-semibold'
									: 'hover:bg-primary/10'}"
								onclick={() => (sidebar.open = false)}
							>
								<NavIcon name={icon} />
								{t(label)}
								{#if path === '/inbox' && inboxLive.count > 0}
									<span class="badge badge-primary badge-sm ml-auto" aria-label={t('nav-inbox-count', { count: inboxLive.count })}>{inboxLive.count}</span>
								{/if}
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
					<!-- svelte-ignore a11y_autofocus -->
					<input
						class="input input-sm w-full"
						autofocus
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
	<div class="relative isolate flex h-dvh min-w-0 flex-1 flex-col overflow-clip">
		<div class="pointer-events-none absolute -right-24 -top-28 h-96 w-96 rounded-full bg-primary/10 blur-3xl"></div>
		<!-- Mobile top bar with the menu toggle -->
		<div class="lg:hidden sticky top-0 z-20 h-14 flex items-center gap-2 px-3 bg-base-200/85 backdrop-blur-xl border-b border-base-300">
			<button class="btn btn-ghost btn-sm" onclick={() => (sidebar.open = true)} aria-label={t('nav-open-menu')}>
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><line x1="4" y1="6" x2="20" y2="6"/><line x1="4" y1="12" x2="20" y2="12"/><line x1="4" y1="18" x2="20" y2="18"/></svg>
			</button>
			<BrandLockup />
		</div>

		<main class="relative z-10 min-h-0 min-w-0 flex-1 {bounded ? 'overflow-hidden' : 'overflow-y-auto'}">
			<div class="w-full {bounded ? 'h-full px-4 py-3 sm:px-6' : 'px-4 pb-8 pt-6 sm:px-6'}">
				{#if featureOff && routeFeature}
					<!-- The URL still resolves — the feature behind it does not.
					     Say which one, so an operator knows which switch to flip. -->
					<div class="w-full max-w-2xl">
						<h1 class="text-2xl font-bold">{t(`settings-s-${routeFeature.replaceAll('.', '-')}`)}</h1>
						<div class="alert alert-warning mt-4">
							<span>{t('feature-disabled-body', { feature: t(`settings-s-${routeFeature.replaceAll('.', '-')}`) })}</span>
						</div>
						{#if isAdmin}
							<a class="btn btn-sm mt-4" href="/admin/settings">{t('feature-disabled-settings-link')}</a>
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
