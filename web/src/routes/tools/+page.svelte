<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiError } from '$lib/api';

	interface ToolEntry {
		key: string;
		title: string;
		description: string;
		category: string;
		enabled: boolean;
	}

	let tools = $state<ToolEntry[]>([]);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	/** Key of the toggle with an in-flight write — disables it while saving. */
	let saving = $state<string | null>(null);

	const sections = $derived.by(() => {
		const byCategory = new Map<string, ToolEntry[]>();
		for (const tool of tools) {
			const list = byCategory.get(tool.category) ?? [];
			list.push(tool);
			byCategory.set(tool.category, list);
		}
		return [...byCategory.entries()];
	});

	async function refresh() {
		try {
			const res = await fetch('/api/v0/tools', { headers: { accept: 'application/json' } });
			if (!res.ok) throw new ApiError(res.status, `tools ${res.status}`);
			const body = (await res.json()) as { tools: ToolEntry[] };
			tools = body.tools;
		} catch (err) {
			error = String(err);
		}
	}

	async function toggle(tool: ToolEntry) {
		if (saving) return;
		saving = tool.key;
		notice = null;
		try {
			const res = await fetch('/api/v0/tools/toggle', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ tool_key: tool.key, enabled: !tool.enabled })
			});
			if (!res.ok) {
				const raw = await res.text();
				throw new Error(raw.slice(0, 200));
			}
			tool.enabled = !tool.enabled;
		} catch (err) {
			notice = String(err);
		} finally {
			saving = null;
		}
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">Tools</h1>
</div>

<p class="text-base-content/60 text-sm mb-6">
	The tools your roles grant. Switching one off hides it from every conversation for your account.
</p>

{#if error}
	<div class="alert alert-error mb-4"><span>{error}</span></div>
{/if}
{#if notice}
	<div class="alert alert-warning mb-4"><span>{notice}</span></div>
{/if}

{#each sections as [category, entries] (category)}
	<div class="card border border-base-300 mb-4">
		<div class="card-body">
			<h2 class="card-title text-base capitalize">{category}</h2>
			<ul class="flex flex-col divide-y divide-base-300">
				{#each entries as tool (tool.key)}
					<li class="py-3 flex items-center gap-4">
						<div class="flex-1 min-w-0">
							<div class="text-sm font-medium">{tool.title}</div>
							<div class="text-xs text-base-content/60">{tool.description}</div>
						</div>
						<input
							type="checkbox"
							class="toggle toggle-primary"
							checked={tool.enabled}
							disabled={saving === tool.key}
							onchange={() => toggle(tool)}
							aria-label="Toggle {tool.title}"
						/>
					</li>
				{/each}
			</ul>
		</div>
	</div>
{:else}
	{#if !error}
		<div class="card border border-base-300">
			<div class="card-body">
				<p class="text-base-content/60 text-sm">No tools granted to your roles.</p>
			</div>
		</div>
	{/if}
{/each}
