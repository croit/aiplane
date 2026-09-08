<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';
	import { base } from '$app/paths';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api, loginUrl } from '$lib/api';
	import { loadMe, me } from '$lib/session.svelte';
	import { feedback, loadConfig, openDialog, submit } from '$lib/feedback.svelte';

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
		void loadConfig();
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
			<a href="{base}/skills" class="btn btn-ghost btn-sm">Skills</a>
			<a href="{base}/integrations" class="btn btn-ghost btn-sm">Integrations</a>
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
