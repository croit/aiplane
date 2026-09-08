<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPut, adminDelete } from '$lib/admin-client';

	interface Skill { name: string; title: string; description: string; }
	interface Data { skills: Skill[]; grants: { skill: string; roles: string[] }[]; groups: string[]; }

	let data = $state<Data | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let editing = $state<string | null>(null);
	let editRoles = $state('');

	async function refresh() {
		try {
			data = await adminJson<Data>('/api/v0/admin/skills');
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function upload(e: Event) {
		const input = e.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		if (!file) return;
		const fd = new FormData();
		fd.append('file', file);
		try {
			const res = await fetch('/api/v0/admin/skills', { method: 'POST', body: fd });
			if (!res.ok) throw new Error((await res.text()).slice(0, 200));
			notice = `Installed ${file.name}.`;
			await refresh();
		} catch (err) {
			notice = String(err);
		} finally {
			input.value = '';
		}
	}

	async function saveGrants(skill: string) {
		try {
			await adminPut('/api/v0/admin/skills/grants', {
				skill,
				roles: editRoles.split(',').map((s) => s.trim()).filter(Boolean)
			});
			editing = null;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function remove(name: string) {
		if (!confirm(`Remove the global skill ${name} and its grants?`)) return;
		try {
			await adminDelete(`/api/v0/admin/skills/${encodeURIComponent(name)}`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">Skills</h1>
	<label class="btn btn-primary btn-sm cursor-pointer">
		Upload .skill
		<input type="file" accept=".skill,.zip" class="hidden" onchange={upload} />
	</label>
</div>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

<ul class="flex flex-col gap-2">
	{#each data?.skills ?? [] as skill (skill.name)}
		{@const grants = data?.grants.find((g) => g.skill === skill.name)}
		<li class="card border border-base-300">
			<div class="card-body py-2">
				<div class="flex items-center gap-3 flex-wrap">
					<span class="font-medium">{skill.title}</span>
					<span class="font-mono text-xs text-base-content/50">{skill.name}</span>
					<span class="flex-1"></span>
					{#if editing === skill.name}
						<input class="input input-bordered input-sm w-64" bind:value={editRoles} placeholder="role-a, role-b" />
						<button class="btn btn-primary btn-xs" onclick={() => saveGrants(skill.name)}>Save</button>
						<button class="btn btn-ghost btn-xs" onclick={() => (editing = null)}>Cancel</button>
					{:else}
						<span class="text-xs text-base-content/60">
							{grants?.roles.length ? grants.roles.join(', ') : 'no extra grants'}
						</span>
						<button class="btn btn-ghost btn-xs" onclick={() => { editing = skill.name; editRoles = (grants?.roles ?? []).join(', '); }}>Grants</button>
						<button class="btn btn-ghost btn-xs text-error" onclick={() => remove(skill.name)}>Delete</button>
					{/if}
				</div>
				<p class="text-xs text-base-content/60">{skill.description}</p>
			</div>
		</li>
	{/each}
</ul>
