<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPut, adminPost, adminDelete } from '$lib/admin-client';

	interface Connector {
		key: string; title: string; description: string | null; base_url: string;
		auth_type: string; scopes: string[]; enabled: boolean; audit: boolean; groups: string[];
	}
	let connectors = $state<Connector[]>([]);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let showForm = $state(false);
	let fkey = $state(''); let ftitle = $state(''); let furl = $state('');
	let fauth = $state('oauth2'); let fscopes = $state(''); let fgroups = $state('');
	let fsecret = $state(''); let overwrite = $state(false);

	async function refresh() {
		try {
			const data = await adminJson<{ connectors: Connector[] }>('/api/v0/admin/connectors');
			connectors = data.connectors;
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function save() {
		try {
			await adminPut('/api/v0/admin/connectors', {
				key: fkey, title: ftitle, base_url: furl, auth_type: fauth,
				scopes: fscopes.split(',').map((s) => s.trim()).filter(Boolean),
				groups: fgroups.split(',').map((s) => s.trim()).filter(Boolean),
				client_secret: fsecret, overwrite
			});
			showForm = false; fkey = ''; ftitle = ''; furl = ''; fscopes = ''; fgroups = ''; fsecret = ''; overwrite = false;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function toggle(c: Connector) {
		await adminPost(`/api/v0/admin/connectors/${encodeURIComponent(c.key)}/toggle`, { enabled: !c.enabled });
		await refresh();
	}

	async function remove(key: string) {
		if (!confirm(`Delete connector ${key} and every user connection to it?`)) return;
		await adminDelete(`/api/v0/admin/connectors/${encodeURIComponent(key)}`);
		await refresh();
	}

	async function restore() {
		if (!confirm('Re-seed the built-in catalog entries?')) return;
		await adminPost('/api/v0/admin/connectors/restore-defaults');
		await refresh();
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4 flex-wrap gap-2">
	<h1 class="text-2xl font-bold">Connectors</h1>
	<div class="flex gap-2">
		<button class="btn btn-ghost btn-sm" onclick={restore}>Restore defaults</button>
		<button class="btn btn-primary btn-sm" onclick={() => (showForm = !showForm)}>New connector</button>
	</div>
</div>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

{#if showForm}
	<div class="card border border-base-300 mb-6">
		<div class="card-body">
			<div class="grid sm:grid-cols-2 gap-3">
				<label class="flex flex-col gap-1"><span class="label-text">Key</span><input class="input input-bordered input-sm" bind:value={fkey} /></label>
				<label class="flex flex-col gap-1"><span class="label-text">Title</span><input class="input input-bordered input-sm" bind:value={ftitle} /></label>
				<label class="flex flex-col gap-1"><span class="label-text">Base URL</span><input class="input input-bordered input-sm" bind:value={furl} placeholder="https://mcp.example.com/mcp" /></label>
				<label class="flex flex-col gap-1"><span class="label-text">Auth</span>
					<select class="select select-bordered select-sm" bind:value={fauth}>
						<option value="oauth2">OAuth 2.1</option>
						<option value="static_bearer">Static bearer</option>
						<option value="none">None</option>
					</select></label>
				<label class="flex flex-col gap-1"><span class="label-text">Scopes (csv)</span><input class="input input-bordered input-sm" bind:value={fscopes} /></label>
				<label class="flex flex-col gap-1"><span class="label-text">Groups (csv, empty = all)</span><input class="input input-bordered input-sm" bind:value={fgroups} /></label>
				<label class="flex flex-col gap-1"><span class="label-text">Client secret / token</span><input class="input input-bordered input-sm" type="password" bind:value={fsecret} /></label>
				<label class="label cursor-pointer gap-2 self-end">
					<input type="checkbox" class="checkbox checkbox-sm" bind:checked={overwrite} />
					<span class="label-text text-xs">overwrite existing</span>
				</label>
			</div>
			<div class="card-actions justify-end mt-2">
				<button class="btn btn-primary btn-sm" onclick={save} disabled={!fkey.trim() || !furl.trim()}>Save</button>
			</div>
		</div>
	</div>
{/if}

<ul class="flex flex-col gap-2">
	{#each connectors as c (c.key)}
		<li class="card border border-base-300">
			<div class="card-body py-2 flex-row items-center gap-3 flex-wrap">
				{#if c.enabled}<span class="badge badge-success badge-sm">enabled</span>{:else}<span class="badge badge-ghost badge-sm">off</span>{/if}
				<span class="font-medium">{c.title}</span>
				<span class="font-mono text-xs text-base-content/50 truncate max-w-56">{c.base_url}</span>
				<span class="badge badge-outline badge-sm">{c.auth_type}</span>
				<span class="flex-1"></span>
				<button class="btn btn-ghost btn-xs" onclick={() => toggle(c)}>{c.enabled ? 'Disable' : 'Enable'}</button>
				<button class="btn btn-ghost btn-xs text-error" onclick={() => remove(c.key)}>Delete</button>
			</div>
		</li>
	{/each}
</ul>
