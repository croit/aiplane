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

	function setTheme(theme: 'light' | 'dark') {
		document.documentElement.dataset.theme = theme;
		// Same cookie the server-rendered chrome reads (session_core::chrome),
		// so a hop between SPA and legacy pages keeps the theme.
		document.cookie = `theme=${theme}; path=/; max-age=31536000; samesite=lax`;
	}

	async function signOut() {
		await api.logout();
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
	let workspaceOpen = $state(true);
	let accountOpen = $state(true);
	let adminOpen = $state(true);

	const workspaceLinks: [string, string][] = [
		['Memory', '/memory'],
		['Scheduled', '/scheduled'],
		['Webhooks', '/webhooks'],
		['Integrations', '/integrations'],
		['My Skills', '/skills'],
		['Tools', '/tools']
	];
	const accountLinks: [string, string][] = [
		['Tokens', '/tokens'],
		['Usage', '/usage']
	];
	const adminLinks: [string, string][] = [
		['Users', '/admin/users'],
		['API tokens', '/admin/tokens'],
		['Groups', '/admin/groups'],
		['Upstreams', '/admin/upstreams'],
		['Models', '/admin/models'],
		['RAG', '/admin/rag'],
		['Skills', '/admin/skills'],
		['Connectors', '/admin/connectors'],
		['ComfyUI', '/admin/comfyui'],
		['Limits', '/admin/limits'],
		['Settings', '/admin/settings']
	];

	const isAdmin = $derived(me.value?.role_ids?.includes('admin') ?? false);

	function isActive(path: string): boolean {
		return page.url.pathname === `${base}${path}` || page.url.pathname === path;
	}

	function isChatActive(): boolean {
		return page.url.pathname.startsWith(`${base}/chat`);
	}

	onMount(() => {
		void refreshSidebar();
		void loadConfig();
		if ('serviceWorker' in navigator) {
			void navigator.serviceWorker.register(`${base}/sw.js`);
		}
	});

	// ---- language switcher (same /lang POST the legacy chrome uses) -------
	let langOpen = $state(false);
	const LANGS: [string, string, string][] = [
		['en', '🇬🇧', 'English'],
		['de', '🇩🇪', 'Deutsch'],
		['fr', '🇫🇷', 'Français'],
		['es', '🇪🇸', 'Español'],
		['ru', '🇷🇺', 'Русский'],
		['zh', '🇨🇳', '中文']
	];
	function setLang(code: string) {
		langOpen = false;
		const next = encodeURIComponent(page.url.pathname + page.url.search);
		window.location.href = `/lang?lang=${code}&next=${next}`;
	}
</script>

<div class="min-h-dvh bg-base-100 text-base-content flex">
	<!-- Mobile backdrop -->
	{#if sidebar.open}
		<button
			class="fixed inset-0 z-30 bg-black/50 lg:hidden"
			aria-label="Close menu"
			onclick={() => (sidebar.open = false)}
		></button>
	{/if}

	<!-- Sidebar -->
	<aside
		class="fixed lg:sticky top-0 z-40 h-dvh w-72 shrink-0 flex flex-col bg-base-200 border-r border-base-300
			transition-transform -translate-x-full lg:translate-x-0 {sidebar.open ? 'translate-x-0' : ''}"
		aria-label="Main navigation"
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
					if (name === 'Workspace') workspaceOpen = !workspaceOpen;
					else if (name === 'Account') accountOpen = !accountOpen;
					else adminOpen = !adminOpen;
				}}
				{@const isOpen = name === 'Workspace'
					? workspaceOpen
					: name === 'Account'
						? accountOpen
						: adminOpen}
				<button
					class="w-full flex items-center gap-1 rounded-lg px-3 pt-4 pb-1 text-[11px] font-semibold tracking-wider text-base-content/50 uppercase hover:bg-base-300/40"
					onclick={toggle}
					aria-label="Toggle {name} section"
				>
					{name}
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
								{label}
							</a>
						{/each}
					</div>
				{/if}
			{/snippet}

			{@render group('Workspace', workspaceOpen, workspaceLinks)}
			{@render group('Account', accountOpen, accountLinks)}
			{#if isAdmin}
				{@render group('Admin', adminOpen, adminLinks)}
			{/if}
		</nav>

		<!-- Conversations -->
		<div class="border-t border-base-300 px-2 py-2">
			<div class="flex items-center justify-between px-2 py-1">
				<span class="text-[11px] font-semibold tracking-wider text-base-content/50 uppercase">Conversations</span>
				<div class="flex gap-1">
					{#if sidebar.searching}
						<button class="btn btn-ghost btn-xs" onclick={closeSearch} aria-label="Close search">✕</button>
					{:else}
						<button class="btn btn-ghost btn-xs" onclick={openSearch} aria-label="Search conversations">
							<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/></svg>
						</button>
					{/if}
					<button class="btn btn-ghost btn-xs" onclick={newChat} aria-label="New conversation" title="New conversation">
						<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
					</button>
				</div>
			</div>

			{#if sidebar.searching}
				<div class="px-2 pb-1">
					<input
						class="input input-sm input-bordered w-full"
						placeholder="Search…"
						value={sidebar.query}
						oninput={(e) => searchAsYouType((e.currentTarget as HTMLInputElement).value)}
						aria-label="Search conversations"
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
							{s.title?.trim() || 'Untitled chat'}
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

		<!-- User footer -->
		<div class="border-t border-base-300 px-3 py-2 flex items-center gap-2">
			<span class="text-xs truncate flex-1 min-w-0" title={me.value?.email ?? ''}>
				{me.value?.email ?? ''}
			</span>
			<div class="relative">
				<button class="btn btn-ghost btn-xs" onclick={() => (langOpen = !langOpen)} aria-label="Choose language" title="Language">
					🇬🇧
				</button>
				{#if langOpen}
					<ul class="absolute bottom-9 right-0 z-50 menu bg-base-200 rounded-box border border-base-300 shadow p-1 w-36">
						{#each LANGS as [code, flag, label] (code)}
							<li>
								<button class="text-sm" onclick={() => setLang(code)}>
									<span aria-hidden="true">{flag}</span> {label}
								</button>
							</li>
						{/each}
					</ul>
				{/if}
			</div>
			<form action="/theme/toggle" method="post" onsubmit={(e) => { e.preventDefault(); setTheme(document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark'); }}>
				<button class="btn btn-ghost btn-xs" title="Toggle theme" aria-label="Toggle theme">
					<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/></svg>
				</button>
			</form>
			<form action="/auth/logout" method="post" onsubmit={(e) => { e.preventDefault(); void signOut(); }}>
				<button class="btn btn-ghost btn-xs" title="Sign out" aria-label="Sign out">
					<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/></svg>
				</button>
			</form>
		</div>
	</aside>

	<!-- Main column -->
	<div class="flex-1 min-w-0 flex flex-col min-h-dvh">
		<!-- Mobile top bar with the menu toggle -->
		<div class="lg:hidden sticky top-0 z-20 h-14 flex items-center gap-2 px-3 bg-base-200 border-b border-base-300">
			<button class="btn btn-ghost btn-sm" onclick={() => (sidebar.open = true)} aria-label="Open menu">
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

	{#if feedback.enabled && me.value}
		<button class="btn btn-circle btn-neutral fixed bottom-4 right-4 z-40" onclick={openDialog} aria-label="Send feedback">
			?
		</button>
	{/if}

	{#if feedback.open}
		<dialog class="modal modal-open" aria-label="Send feedback">
			<div class="modal-box max-w-lg">
				{#if feedback.submitted}
					<h2 class="text-lg font-semibold mb-2">Thank you</h2>
					<p class="text-sm text-base-content/70">Your feedback was filed as an issue.</p>
					<div class="modal-action"><button class="btn btn-primary btn-sm" onclick={() => (feedback.open = false)}>Done</button></div>
				{:else}
					<h2 class="text-lg font-semibold mb-3">Send feedback</h2>
					<div class="flex flex-col gap-3">
						<input class="input input-bordered input-sm" placeholder="Short summary" bind:value={feedback.title} />
						<textarea class="textarea textarea-bordered text-sm" rows="3" placeholder="What happened, or what would you like?" bind:value={feedback.description}></textarea>
						<textarea class="textarea textarea-bordered text-sm" rows="2" placeholder="Why does this matter?" bind:value={feedback.business}></textarea>
						<textarea class="textarea textarea-bordered text-sm" rows="2" placeholder="When is this done?" bind:value={feedback.acceptance}></textarea>
						<select class="select select-bordered select-sm" bind:value={feedback.priority}>
							<option value="low">Low</option><option value="medium">Medium</option><option value="high">High</option>
						</select>
						{#if feedback.error}<div class="alert alert-error py-2 text-sm"><span>{feedback.error}</span></div>{/if}
					</div>
					<div class="modal-action">
						<button class="btn btn-ghost btn-sm" onclick={() => (feedback.open = false)}>Cancel</button>
						<button class="btn btn-primary btn-sm" onclick={submit} disabled={feedback.busy || !feedback.title.trim() || !feedback.description.trim()}>
							{feedback.busy ? 'Sending…' : 'Send'}
						</button>
					</div>
				{/if}
			</div>
			<form method="dialog" class="modal-backdrop"><button onclick={() => (feedback.open = false)}>close</button></form>
		</dialog>
	{/if}
</div>
