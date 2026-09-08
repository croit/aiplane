<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost, adminPut, adminDelete } from '$lib/admin-client';

	interface Action {
		id: string;
		name: string;
		prompt: string;
		model: string;
		cron: string;
		timezone: string;
		tools_enabled: boolean;
		reuse_conversation: boolean;
		enabled: boolean;
		next_run_at: string | null;
		last_session_id: string | null;
		last_status: string | null;
	}
	interface Data {
		actions: Action[];
		models: string[];
	}

	let data = $state<Data | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let editing = $state<string | null>(null);

	let fname = $state('');
	let fprompt = $state('');
	let fmodel = $state('');
	let fcron = $state('');
	let preview = $state<string | null>(null);

	async function refresh() {
		try {
			data = await adminJson<Data>('/api/v0/scheduled');
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function runPreview() {
		if (!fcron.trim()) {
			preview = null;
			return;
		}
		try {
			const res = await adminPost<{ summary: string; upcoming: string[] }>(
				'/api/v0/scheduled/preview',
				{ cron: fcron }
			);
			preview = `${res.summary} — next: ${res.upcoming.map((u) => new Date(u).toLocaleString()).join(', ')}`;
		} catch (err) {
			preview = String(err);
		}
	}

	async function save() {
		notice = null;
		try {
			const body = {
				name: fname,
				prompt: fprompt,
				model: fmodel,
				cron: fcron,
				tools_enabled: false,
				reuse_conversation: false,
				reuse_rounds: 5
			};
			if (editing) await adminPut(`/api/v0/scheduled/${editing}`, body);
			else await adminPost('/api/v0/scheduled', body);
			editing = null;
			fname = ''; fprompt = ''; fcron = '';
			preview = null;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function toggle(a: Action) {
		try {
			await adminPost(`/api/v0/scheduled/${a.id}/toggle`, { enabled: !a.enabled });
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function remove(id: string) {
		if (!confirm('Delete this scheduled action?')) return;
		try {
			await adminDelete(`/api/v0/scheduled/${id}`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">Scheduled</h1>
</div>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

<div class="card border border-base-300 mb-6">
	<div class="card-body">
		<h2 class="card-title text-base">{editing ? `Edit ${editing}` : 'New scheduled action'}</h2>
		<div class="grid sm:grid-cols-2 gap-3">
			<label class="flex flex-col gap-1"><span class="label-text">Name</span>
				<input class="input input-bordered input-sm" bind:value={fname} /></label>
			<label class="flex flex-col gap-1"><span class="label-text">Model</span>
				<input class="input input-bordered input-sm" bind:value={fmodel} list="sched-models" />
				<datalist id="sched-models">{#each data?.models ?? [] as m (m)}<option value={m}></option>{/each}</datalist>
			</label>
			<label class="flex flex-col gap-1 sm:col-span-2"><span class="label-text">Prompt</span>
				<textarea class="textarea textarea-bordered text-sm" rows="3" bind:value={fprompt}></textarea></label>
			<label class="flex flex-col gap-1"><span class="label-text">Cron (5-field)</span>
				<input class="input input-bordered input-sm font-mono" bind:value={fcron} oninput={() => setTimeout(runPreview, 400)} placeholder="0 9 * * 1-5" /></label>
			{#if preview}<p class="text-xs text-base-content/60 self-end">{preview}</p>{/if}
		</div>
		<div class="card-actions justify-end mt-2">
			{#if editing}<button class="btn btn-ghost btn-sm" onclick={() => { editing = null; fname=''; fprompt=''; fcron=''; }}>Cancel</button>{/if}
			<button class="btn btn-primary btn-sm" onclick={save} disabled={!fname.trim() || !fprompt.trim() || !fcron.trim() || !fmodel.trim()}>Save</button>
		</div>
	</div>
</div>

<ul class="flex flex-col gap-3">
	{#each data?.actions ?? [] as a (a.id)}
		<li class="card border border-base-300">
			<div class="card-body py-3">
				<div class="flex items-center gap-3 flex-wrap">
					{#if a.enabled}<span class="badge badge-success badge-sm">active</span>{:else}<span class="badge badge-ghost badge-sm">paused</span>{/if}
					<span class="font-medium">{a.name}</span>
					<span class="font-mono text-xs text-base-content/50">{a.cron}</span>
					{#if a.next_run_at}
						<span class="text-xs text-base-content/60">next {new Date(a.next_run_at).toLocaleString()}</span>
					{/if}
					{#if a.last_status === 'error'}<span class="badge badge-error badge-sm">last run failed</span>{/if}
					<span class="flex-1"></span>
					<button class="btn btn-ghost btn-xs" onclick={() => toggle(a)}>{a.enabled ? 'Pause' : 'Resume'}</button>
					<button class="btn btn-ghost btn-xs" onclick={() => { editing = a.id; fname = a.name; fprompt = a.prompt; fmodel = a.model; fcron = a.cron; }}>Edit</button>
					<button class="btn btn-ghost btn-xs text-error" onclick={() => remove(a.id)}>Delete</button>
				</div>
				<p class="text-xs text-base-content/60 line-clamp-2">{a.prompt}</p>
			</div>
		</li>
	{/each}
</ul>
