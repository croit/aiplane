<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost } from '$lib/admin-client';

	interface Workflow {
		id: string;
		title: string;
		description: string;
	}
	let workflows = $state<Workflow[]>([]);
	let meta = $state<{ base_url?: string; content_dir?: string } | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	async function refresh() {
		try {
			const data = await adminJson<{ workflows: Workflow[]; base_url?: string; content_dir?: string }>(
				'/api/v0/comfyui/catalog'
			);
			workflows = data.workflows ?? [];
			meta = data;
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function reload() {
		try {
			const res = await adminPost<{ report: { loaded: number; errors: string[] } }>(
				'/api/v0/comfyui/reload'
			);
			notice = `Reloaded: ${res.report?.loaded ?? 0} workflow(s).`;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4 flex-wrap gap-2">
	<h1 class="text-2xl font-bold">ComfyUI</h1>
	<button class="btn btn-primary btn-sm" onclick={reload}>Reload catalog</button>
</div>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

<ul class="flex flex-col gap-2">
	{#each workflows as w (w.id)}
		<li class="card border border-base-300">
			<div class="card-body py-2">
				<span class="font-medium">{w.title}</span>
				<p class="text-xs text-base-content/60">{w.description}</p>
			</div>
		</li>
	{:else}
		<li class="text-base-content/60 text-sm">No workflows loaded — check the content directory.</li>
	{/each}
</ul>
