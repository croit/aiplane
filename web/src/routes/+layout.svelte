<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';
	import { base } from '$app/paths';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api, loginUrl } from '$lib/api';
	import { loadMe, me } from '$lib/session.svelte';

	let { children } = $props();

	loadMe();

	// Signed out (401 from /api/v0/me) anywhere in the SPA → start the OIDC
	// login, coming back to the route the user actually wanted.
	$effect(() => {
		if (me.loaded && me.value === null && !page.url.pathname.endsWith('/login')) {
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
		// The endpoint 303s to `/`; we stay in the SPA and bounce to the
		// login page instead (which will redirect into the OIDC flow).
		await goto('/login', { keepFocus: false });
	}

	let openMenu = $state(false);

	// The SPA's service worker (scope /app/): Web Push notifications +
	// PWA installability. See static/sw.js.
	onMount(() => {
		if ('serviceWorker' in navigator) {
			void navigator.serviceWorker.register(`${base}/sw.js`);
		}
	});
</script>

<div class="min-h-dvh bg-base-100 text-base-content">
	<header class="navbar border-b border-base-300 bg-base-200 px-4">
		<div class="flex-1 gap-2">
			<a href="{base}" class="text-lg font-semibold">LLM Gateway</a>
			<a href="{base}/chat" class="btn btn-ghost btn-sm">Chat</a>
			<a href="{base}/memory" class="btn btn-ghost btn-sm">Memory</a>
			<a href="{base}/scheduled" class="btn btn-ghost btn-sm">Scheduled</a>
			<a href="{base}/webhooks" class="btn btn-ghost btn-sm">Webhooks</a>
			<a href="{base}/tokens" class="btn btn-ghost btn-sm">Tokens</a>
			<a href="{base}/usage" class="btn btn-ghost btn-sm">Usage</a>
			<a href="{base}/tools" class="btn btn-ghost btn-sm">Tools</a>
			{#if me.value?.role_ids?.includes('admin')}
				<a href="{base}/admin/groups" class="btn btn-ghost btn-sm">Admin</a>
			{/if}
			{#if me.value}
				<span class="badge badge-outline badge-sm">{me.value.email}</span>
			{/if}
		</div>
		<div class="flex-none gap-1">
			<div class="dropdown dropdown-end">
				<button class="btn btn-ghost btn-sm" onclick={() => (openMenu = !openMenu)}>
					Theme
				</button>
				{#if openMenu}
					<!-- Close on any navigation or outside click via the backdrop. -->
					<button
						class="fixed inset-0 z-10 cursor-default bg-transparent"
						onclick={() => (openMenu = false)}
						aria-label="Close menu"
					></button>
					<ul class="dropdown-content z-20 menu rounded-box bg-base-200 p-2 shadow">
						<li><button onclick={() => setTheme('light')}>Light</button></li>
						<li><button onclick={() => setTheme('dark')}>Dark</button></li>
					</ul>
				{/if}
			</div>
			{#if me.value}
				<button class="btn btn-ghost btn-sm" onclick={signOut}>Sign out</button>
			{/if}
		</div>
	</header>

	<main class="mx-auto max-w-3xl p-4">
		{@render children()}
	</main>
</div>
