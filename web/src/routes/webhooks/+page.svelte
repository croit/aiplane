<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { base } from '$app/paths';
	import { adminJson, adminPost, adminPut, adminDelete } from '$lib/admin-client';
	import { t, dt } from '$lib/i18n.svelte';

	interface Hook {
		id: string;
		name: string;
		prompt: string;
		model: string;
		tools_enabled: boolean;
		synchronous: boolean;
		enabled: boolean;
	}
	interface Run {
		id: string;
		status: string;
		error: string | null;
		fired_at: string;
		source: string;
		session_id: string | null;
		prompt: string | null;
	}
	interface Data {
		webhooks: Hook[];
		models: string[];
	}

	let data = $state<Data | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let editing = $state<string | null>(null);
	let secret = $state<string | null>(null);
	let fname = $state('');
	let fprompt = $state('');
	let fmodel = $state('');
	// Run history + rerun composer for whichever webhook is expanded.
	let runsFor = $state<string | null>(null);
	let runs = $state<Run[]>([]);
	let rerunPrompt = $state('');
	let rerunning = $state(false);

	async function refresh() {
		try {
			data = await adminJson<Data>('/api/v0/webhooks');
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function save() {
		notice = null;
		try {
			const body = { name: fname, prompt: fprompt, model: fmodel };
			if (editing) await adminPut(`/api/v0/webhooks/${editing}`, body);
			else {
				const res = await adminPost<{ secret: string }>('/api/v0/webhooks', body);
				secret = res.secret;
			}
			editing = null;
			fname = ''; fprompt = '';
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function toggle(h: Hook) {
		await adminPost(`/api/v0/webhooks/${h.id}/toggle`, { enabled: !h.enabled });
		await refresh();
	}

	async function rotate(h: Hook) {
		if (!confirm(t('webhooks-rotate-confirm'))) return;
		try {
			const res = await adminPost<{ secret: string }>(`/api/v0/webhooks/${h.id}/rotate`);
			secret = res.secret;
		} catch (err) {
			notice = String(err);
		}
	}

	async function remove(id: string) {
		if (!confirm(t('webhooks-delete-confirm'))) return;
		await adminDelete(`/api/v0/webhooks/${id}`);
		await refresh();
	}

	/** Show a webhook's recent fires, each linking to the chat it produced. */
	async function showRuns(h: Hook) {
		if (runsFor === h.id) {
			runsFor = null;
			return;
		}
		runsFor = h.id;
		rerunPrompt = h.prompt;
		try {
			runs = (await adminJson<{ runs: Run[] }>(`/api/v0/webhooks/${h.id}/runs`)).runs ?? [];
		} catch (err) {
			runs = [];
			notice = String(err);
		}
	}

	/** Replay a stored payload through a (usually different) prompt. The run
	 * completes server-side before answering, so this waits. */
	async function rerun(h: Hook, runId: string | null) {
		const prompt = rerunPrompt.trim();
		if (!prompt) return;
		rerunning = true;
		notice = null;
		try {
			const res = await adminPost<{ session_id: string; status: string; error: string | null }>(
				`/api/v0/webhooks/${h.id}/rerun`,
				{ prompt, run: runId }
			);
			notice =
				res.status === 'ok'
					? t('webhooks-toast-rerun-started')
					: t('webhooks-toast-rerun-failed', { status: res.status }) +
						(res.error ? `: ${res.error}` : '');
			if (res.status === 'ok') await goto(`${base}/chat/${res.session_id}`);
			else await showRuns(h);
		} catch (err) {
			notice = String(err);
		} finally {
			rerunning = false;
		}
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">{t('webhooks-heading')}</h1>
</div>

<p class="text-base-content/60 text-sm mb-6">
	{t('webhooks-intro')}
</p>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

{#if secret}
	<div class="card border border-success mb-6">
		<div class="card-body">
			<h2 class="card-title text-base">{t('webhooks-reveal-heading')}</h2>
			<p class="text-sm text-base-content/70">{t('webhooks-reveal-note')}</p>
			<pre class="bg-base-100 border border-base-300 rounded-md p-3 font-mono text-xs select-all break-all whitespace-pre-wrap">curl -X POST "{location.origin}/hooks/{secret}" -d 'your payload'</pre>
		</div>
	</div>
{/if}

<div class="card border border-base-300 mb-6">
	<div class="card-body">
		<h2 class="card-title text-base">
			{editing ? t('webhooks-edit-heading') : t('webhooks-new-heading')}
		</h2>
		<div class="grid sm:grid-cols-2 gap-3">
			<label class="flex flex-col gap-1"><span class="label-text">{t('webhooks-name-label')}</span>
				<input
					class="input input-bordered input-sm"
					bind:value={fname}
					placeholder={t('webhooks-name-placeholder')}
				/></label>
			<label class="flex flex-col gap-1"><span class="label-text">{t('webhooks-model-label')}</span>
				<input
					class="input input-bordered input-sm"
					bind:value={fmodel}
					list="hook-models"
					placeholder={t('webhooks-model-placeholder')}
				/>
				<datalist id="hook-models">{#each data?.models ?? [] as m (m)}<option value={m}></option>{/each}</datalist>
			</label>
			<label class="flex flex-col gap-1 sm:col-span-2"><span class="label-text">{t('webhooks-prompt-untrusted-label')}</span>
				<textarea
					class="textarea textarea-bordered text-sm"
					rows="3"
					bind:value={fprompt}
					placeholder={t('webhooks-prompt-placeholder')}
				></textarea></label>
		</div>
		<div class="card-actions justify-end mt-2">
			{#if editing}<button class="btn btn-ghost btn-sm" onclick={() => { editing = null; fname=''; fprompt=''; }}>{t('admin-cancel')}</button>{/if}
			<button class="btn btn-primary btn-sm" onclick={save} disabled={!fname.trim() || !fprompt.trim() || !fmodel.trim()}>{t('groups-save')}</button>
		</div>
	</div>
</div>

<ul class="flex flex-col gap-3">
	{#each data?.webhooks ?? [] as h (h.id)}
		<li class="card border border-base-300">
			<div class="card-body py-3">
				<div class="flex items-center gap-3 flex-wrap">
					{#if h.enabled}<span class="badge badge-success badge-sm">{t('webhooks-badge-active')}</span>{:else}<span class="badge badge-ghost badge-sm">{t('webhooks-badge-paused')}</span>{/if}
					<span class="font-medium">{h.name}</span>
					<span class="font-mono text-xs text-base-content/50">{h.model}</span>
					{#if h.synchronous}<span class="badge badge-outline badge-sm">{t('webhooks-mode-sync')}</span>{/if}
					<span class="flex-1"></span>
					<button class="btn btn-ghost btn-xs" onclick={() => toggle(h)}>{h.enabled ? t('webhooks-pause-title') : t('webhooks-resume-title')}</button>
					<button class="btn btn-ghost btn-xs" onclick={() => rotate(h)}>{t('webhooks-rotate-title')}</button>
					<button class="btn btn-ghost btn-xs" onclick={() => showRuns(h)}>
						{runsFor === h.id ? t('webhooks-runs-hide') : t('webhooks-runs-show')}
					</button>
					<button class="btn btn-ghost btn-xs" onclick={() => { editing = h.id; fname = h.name; fprompt = h.prompt; fmodel = h.model; }}>{t('webhooks-edit-title')}</button>
					<button class="btn btn-ghost btn-xs text-error" onclick={() => remove(h.id)}>{t('webhooks-delete-title')}</button>
				</div>
				<p class="text-xs text-base-content/60 line-clamp-2">{h.prompt}</p>

				{#if runsFor === h.id}
					<div class="border-t border-base-300 mt-2 pt-2 flex flex-col gap-2">
						<ul class="flex flex-col gap-1">
							{#each runs as r (r.id)}
								<li class="flex items-center gap-2 text-xs flex-wrap">
									<span
										class="badge badge-xs {r.status === 'ok' ? 'badge-success' : 'badge-error'}"
									>{r.status}</span>
									<span class="text-base-content/50">{dt(r.fired_at)}</span>
									<span class="badge badge-ghost badge-xs">{r.source}</span>
									{#if r.error}<span class="text-error truncate max-w-64">{r.error}</span>{/if}
									<span class="flex-1"></span>
									{#if r.session_id}
										<a class="btn btn-ghost btn-xs" href="{base}/chat/{r.session_id}">{t('webhooks-run-open')}</a>
									{/if}
									<button
										class="btn btn-ghost btn-xs"
										onclick={() => rerun(h, r.id)}
										disabled={rerunning}
									>
										{t('webhooks-run-rerun')}
									</button>
								</li>
							{:else}
								<li class="text-xs text-base-content/50">{t('webhooks-runs-empty')}</li>
							{/each}
						</ul>
						<div class="flex flex-col gap-1">
							<span class="label-text text-xs">
								{t('webhooks-rerun-prompt-label')}
							</span>
							<textarea
								class="textarea textarea-bordered text-sm"
								rows="2"
								bind:value={rerunPrompt}
							></textarea>
							<div class="flex justify-end">
								<button
									class="btn btn-primary btn-xs"
									onclick={() => rerun(h, null)}
									disabled={rerunning || !rerunPrompt.trim()}
								>
									{rerunning ? t('webhooks-rerun-running') : t('webhooks-rerun-latest')}
								</button>
							</div>
						</div>
					</div>
				{/if}
			</div>
		</li>
	{:else}
		<li class="text-sm text-base-content/50">{t('webhooks-list-empty')}</li>
	{/each}
</ul>
