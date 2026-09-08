<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { base } from '$app/paths';
	import { api } from '$lib/api';
	import { push, enablePush, disablePush, refreshPushState } from '$lib/push.svelte';
	import type { ChatSession } from '$lib/chat-protocol';

	let sessions = $state<ChatSession[]>([]);
	let error = $state<string | null>(null);
	let busy = $state(false);

	async function refresh() {
		try {
			const list = await api.listChatSessions();
			sessions = list.sessions;
		} catch (err) {
			error = String(err);
		}
	}

	async function newChat() {
		if (busy) return;
		busy = true;
		try {
			const { session } = await api.createChatSession();
			goto(`${base}/chat/${session.id}`);
		} catch (err) {
			error = String(err);
		} finally {
			busy = false;
		}
	}

	async function remove(id: string) {
		await api.deleteChatSession(id);
		await refresh();
	}

	async function togglePin(session: ChatSession) {
		await api.pinChatSession(session.id, !session.pinned);
		await refresh();
	}

	onMount(() => {
		void refresh();
		void refreshPushState();
	});
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">Chat</h1>
	<button class="btn btn-primary btn-sm" onclick={newChat} disabled={busy}>New chat</button>
</div>

{#if error}
	<div class="alert alert-error mb-4"><span>{error}</span></div>
{/if}

{#if push.ui.phase !== 'unsupported' && push.ui.phase !== 'denied'}
	<div class="card border border-base-300 mb-4">
		<div class="card-body py-3">
			<div class="flex items-center gap-3">
				<span class="text-sm flex-1">
					{push.ui.phase === 'on' ? 'Notifications are on for this device.' : 'Get notified when a reply finishes.'}
					{#if push.note}<span class="text-base-content/60"> {push.note}</span>{/if}
				</span>
				{#if push.ui.phase === 'on'}
					<button class="btn btn-ghost btn-sm" onclick={disablePush}>Disable</button>
				{:else if push.ui.phase === 'off'}
					<button class="btn btn-outline btn-sm" onclick={enablePush}>Enable on this device</button>
				{:else if push.ui.phase === 'busy'}
					<span class="loading loading-spinner loading-sm"></span>
				{/if}
			</div>
		</div>
	</div>
{/if}

{#if sessions.length === 0 && !error}
	<div class="card border border-base-300">
		<div class="card-body">
			<p class="text-base-content/60">No conversations yet. Start one above.</p>
		</div>
	</div>
{/if}

<ul class="flex flex-col gap-2">
	{#each sessions as session (session.id)}
		{@const active = page.url.pathname === `${base}/chat/${session.id}`}
		<li>
			<div
				class="card border {active ? 'border-primary' : 'border-base-300'}"
			>
				<div class="card-body flex-row items-center gap-3 py-3">
					<a href="{base}/chat/{session.id}" class="flex-1 min-w-0 hover:underline">
						<span class="font-medium">{session.title?.trim() || 'Untitled chat'}</span>
						<span class="block text-xs text-base-content/60">
							{new Date(session.updated_at).toLocaleString()}
						</span>
					</a>
					{#if session.pinned}<span class="badge badge-outline badge-sm">pinned</span>{/if}
					<button
						class="btn btn-ghost btn-xs"
						onclick={() => togglePin(session)}
						aria-label={session.pinned ? 'Unpin conversation' : 'Pin conversation'}
					>
						{session.pinned ? 'Unpin' : 'Pin'}
					</button>
					<button
						class="btn btn-ghost btn-xs text-error"
						onclick={() => remove(session.id)}
						aria-label="Delete conversation"
					>
						Delete
					</button>
				</div>
			</div>
		</li>
	{/each}
</ul>
