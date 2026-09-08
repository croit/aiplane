<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost, adminPut, adminDelete } from '$lib/admin-client';

	interface Hook {
		id: string;
		name: string;
		prompt: string;
		model: string;
		tools_enabled: boolean;
		synchronous: boolean;
		enabled: boolean;
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
		if (!confirm('Issue a new trigger secret? The old URL stops working immediately.')) return;
		try {
			const res = await adminPost<{ secret: string }>(`/api/v0/webhooks/${h.id}/rotate`);
			secret = res.secret;
		} catch (err) {
			notice = String(err);
		}
	}

	async function remove(id: string) {
		if (!confirm('Delete this webhook?')) return;
		await adminDelete(`/api/v0/webhooks/${id}`);
		await refresh();
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">Webhooks</h1>
</div>

<p class="text-base-content/60 text-sm mb-6">
	Prompt endpoints: POST any payload to the trigger URL and the stored prompt runs over it (headless).
</p>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

{#if secret}
	<div class="card border border-success mb-6">
		<div class="card-body">
			<h2 class="card-title text-base">Trigger URL — copy it now, it is shown once</h2>
			<pre class="bg-base-100 border border-base-300 rounded-md p-3 font-mono text-xs select-all break-all whitespace-pre-wrap">curl -X POST "{location.origin}/hooks/{secret}" -d 'your payload'</pre>
		</div>
	</div>
{/if}

<div class="card border border-base-300 mb-6">
	<div class="card-body">
		<h2 class="card-title text-base">{editing ? 'Edit webhook' : 'New webhook'}</h2>
		<div class="grid sm:grid-cols-2 gap-3">
			<label class="flex flex-col gap-1"><span class="label-text">Name</span>
				<input class="input input-bordered input-sm" bind:value={fname} /></label>
			<label class="flex flex-col gap-1"><span class="label-text">Model</span>
				<input class="input input-bordered input-sm" bind:value={fmodel} list="hook-models" />
				<datalist id="hook-models">{#each data?.models ?? [] as m (m)}<option value={m}></option>{/each}</datalist>
			</label>
			<label class="flex flex-col gap-1 sm:col-span-2"><span class="label-text">Prompt (the payload rides in as untrusted input)</span>
				<textarea class="textarea textarea-bordered text-sm" rows="3" bind:value={fprompt}></textarea></label>
		</div>
		<div class="card-actions justify-end mt-2">
			{#if editing}<button class="btn btn-ghost btn-sm" onclick={() => { editing = null; fname=''; fprompt=''; }}>Cancel</button>{/if}
			<button class="btn btn-primary btn-sm" onclick={save} disabled={!fname.trim() || !fprompt.trim() || !fmodel.trim()}>Save</button>
		</div>
	</div>
</div>

<ul class="flex flex-col gap-3">
	{#each data?.webhooks ?? [] as h (h.id)}
		<li class="card border border-base-300">
			<div class="card-body py-3">
				<div class="flex items-center gap-3 flex-wrap">
					{#if h.enabled}<span class="badge badge-success badge-sm">enabled</span>{:else}<span class="badge badge-ghost badge-sm">disabled</span>{/if}
					<span class="font-medium">{h.name}</span>
					<span class="font-mono text-xs text-base-content/50">{h.model}</span>
					{#if h.synchronous}<span class="badge badge-outline badge-sm">synchronous</span>{/if}
					<span class="flex-1"></span>
					<button class="btn btn-ghost btn-xs" onclick={() => toggle(h)}>{h.enabled ? 'Disable' : 'Enable'}</button>
					<button class="btn btn-ghost btn-xs" onclick={() => rotate(h)}>Rotate</button>
					<button class="btn btn-ghost btn-xs" onclick={() => { editing = h.id; fname = h.name; fprompt = h.prompt; fmodel = h.model; }}>Edit</button>
					<button class="btn btn-ghost btn-xs text-error" onclick={() => remove(h.id)}>Delete</button>
				</div>
				<p class="text-xs text-base-content/60 line-clamp-2">{h.prompt}</p>
			</div>
		</li>
	{/each}
</ul>
