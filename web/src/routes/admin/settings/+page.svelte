<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost } from '$lib/admin-client';

	interface Field {
		key: string;
		kind: string;
		restart: boolean;
		value: string | null;
		secret_set: boolean;
	}
	interface Section {
		name: string;
		category: string;
		fields: Field[];
	}
	interface SettingsData {
		sections: Section[];
		restart_pending: string[];
	}

	let data = $state<SettingsData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	/** Per-section drafts: section → key → current input value. */
	let drafts = $state<Record<string, Record<string, string>>>({});

	async function refresh() {
		try {
			data = await adminJson<SettingsData>('/api/v0/admin/settings');
			const next: Record<string, Record<string, string>> = {};
			for (const section of data.sections) {
				next[section.name] = {};
				for (const field of section.fields) {
					next[section.name][field.key] = field.kind === 'bool' ? (field.value === 'true' ? 'true' : 'false') : (field.value ?? '');
				}
			}
			drafts = next;
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function save(section: Section) {
		notice = null;
		try {
			await adminPost('/api/v0/admin/settings', {
				section: section.name,
				values: drafts[section.name] ?? {}
			});
			notice = `Saved ${section.name}.`;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function clearField(key: string) {
		if (!confirm(`Reset ${key} to its built-in default?`)) return;
		try {
			await adminPost('/api/v0/admin/settings/clear', { key });
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	const categories = $derived(
		data ? [...new Set(data.sections.map((s) => s.category))] : []
	);

	onMount(refresh);
</script>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}
{#if data && data.restart_pending.length > 0}
	<div class="alert alert-warning mb-4 text-sm">
		<span>Restart pending for: {data.restart_pending.join(', ')}</span>
	</div>
{/if}

{#each categories as category (category)}
	<h2 class="text-lg font-semibold capitalize mb-2 mt-4">{category}</h2>
	{#if data}
		{#each data.sections.filter((s) => s.category === category) as section (section.name)}
			<div class="card border border-base-300 mb-4">
				<div class="card-body">
					<h3 class="card-title text-base font-mono text-sm">{section.name}</h3>
					<div class="grid sm:grid-cols-2 gap-3">
						{#each section.fields as field (field.key)}
							<label class="flex flex-col gap-1">
								<span class="label-text font-mono text-xs break-all">
									{field.key}
									{#if field.restart}<span class="text-warning" title="Takes effect after a restart">⟳</span>{/if}
								</span>
								{#if field.kind === 'bool'}
									<input
										type="checkbox"
										class="toggle toggle-primary"
										checked={(drafts[section.name]?.[field.key] ?? 'false') === 'true'}
										onchange={(e) => {
											drafts[section.name][field.key] = (e.currentTarget as HTMLInputElement).checked ? 'true' : 'false';
										}}
									/>
								{:else if field.kind === 'secret'}
									<div class="join">
										<input
											class="input input-bordered input-sm join-item w-full"
											type="password"
											placeholder={field.secret_set ? '(set — blank keeps)' : ''}
											bind:value={drafts[section.name][field.key]}
										/>
										{#if field.secret_set}
											<button class="btn btn-ghost btn-sm join-item" type="button" onclick={() => clearField(field.key)}>Clear</button>
										{/if}
									</div>
								{:else}
									<input
										class="input input-bordered input-sm font-mono text-xs"
										type={field.kind === 'int' || field.kind === 'float' ? 'number' : 'text'}
										step={field.kind === 'float' ? 'any' : undefined}
										bind:value={drafts[section.name][field.key]}
									/>
								{/if}
							</label>
						{/each}
					</div>
					<div class="card-actions justify-end mt-2">
						<button class="btn btn-primary btn-sm" onclick={() => save(section)}>Save section</button>
					</div>
				</div>
			</div>
		{/each}
	{/if}
{/each}
