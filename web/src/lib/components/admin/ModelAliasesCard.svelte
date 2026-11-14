<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { adminDelete, adminJson, adminPut } from '#lib/admin-client.js';
	import { t } from '#lib/i18n.svelte.js';
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import {
		aliasNameProblem,
		aliasTargetProblem,
		draftFromAlias,
		servedTargetCount,
		type AliasDraft,
		type ModelAlias,
		type ModelAliasesData
	} from '#lib/model-aliases.js';

	let data = $state<ModelAliasesData | null>(null);
	let editing = $state<AliasDraft | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let saving = $state(false);
	let removing = $state<string | null>(null);
	let card: HTMLElement;
	let editor = $state<HTMLFormElement | undefined>();

	async function refresh() {
		try {
			data = await adminJson<ModelAliasesData>('/api/v0/admin/model-aliases');
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	onMount(refresh);

	let realModels = $derived(data ? [...new Set(data.backends.flatMap((backend) => backend.models))] : []);

	function backendModels(name: string): string[] {
		return data?.backends.find((backend) => backend.name === name)?.models ?? [];
	}

	function modelOptions(backend: string, current: string) {
		const models = backendModels(backend);
		const options = [{ value: '', label: t('model-alias-sole-model'), description: t('model-alias-sole-model-hint') }];
		for (const model of models) options.push({ value: model, label: model, description: '' });
		if (current && !models.includes(current)) {
			options.push({ value: current, label: current, description: t('model-alias-unresolved') });
		}
		return options;
	}

	function firstFreeBackend(used: readonly string[]): string {
		return data?.backends.find((backend) => !used.includes(backend.name))?.name ?? '';
	}

	async function showEditor(next: AliasDraft) {
		editing = next;
		notice = null;
		error = null;
		await tick();
		editor?.scrollIntoView({ block: 'start' });
	}

	function startNew() {
		if (!data || data.backends.length === 0) return;
		void showEditor({
			name: '',
			targets: [{ backend: data.backends[0].name, target: '' }]
		});
	}

	function startEdit(alias: ModelAlias) {
		void showEditor(draftFromAlias(alias));
	}

	async function cancel() {
		editing = null;
		await tick();
		card?.scrollIntoView({ block: 'start' });
	}

	function addTarget() {
		if (!editing || !data) return;
		const backend = firstFreeBackend(editing.targets.map((target) => target.backend));
		if (!backend) return;
		editing.targets.push({ backend, target: '' });
	}

	function removeTarget(index: number) {
		editing?.targets.splice(index, 1);
	}

	function changeBackend(target: AliasDraft['targets'][number]) {
		target.target = '';
	}

	async function save() {
		if (!editing) return;
		const name = editing.name.trim();
		const nameProblem = aliasNameProblem(name, realModels);
		if (nameProblem) {
			error = t(nameProblem, { name });
			return;
		}
		const targetProblem = aliasTargetProblem(editing.targets);
		if (targetProblem) {
			error = t(targetProblem);
			return;
		}
		saving = true;
		error = null;
		try {
			await adminPut('/api/v0/admin/model-aliases', {
				name,
				targets: editing.targets.map((target) => ({
					backend: target.backend,
					target: target.target || null
				}))
			});
			notice = t('model-alias-saved', { name });
			editing = null;
			await refresh();
			await tick();
			card?.scrollIntoView({ block: 'start' });
		} catch (caught) {
			error = String(caught);
		} finally {
			saving = false;
		}
	}

	async function remove(alias: ModelAlias) {
		if (!confirm(t('model-alias-delete-confirm', { name: alias.name }))) return;
		removing = alias.name;
		error = null;
		try {
			await adminDelete(`/api/v0/admin/model-aliases/${encodeURIComponent(alias.name)}`);
			notice = t('model-alias-deleted', { name: alias.name });
			await refresh();
		} catch (caught) {
			error = String(caught);
		} finally {
			removing = null;
		}
	}
</script>

<article class="card" bind:this={card}>
	<div class="card-body gap-3 p-3 sm:p-6">
		<header class="flex flex-wrap items-start justify-between gap-3">
			<div class="max-w-3xl">
				<h2 class="card-title text-base">{t('model-alias-heading')}</h2>
				<p class="text-sm text-base-content/70">{t('model-alias-intro')}</p>
			</div>
			<button class="btn btn-primary btn-sm" type="button" onclick={startNew} disabled={!data || data.backends.length === 0}>
				+ {t('model-alias-new')}
			</button>
		</header>

		{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}
		{#if notice}<div class="alert alert-success text-sm" role="status"><span>{notice}</span></div>{/if}

		{#if editing}
			<form class="flex flex-col gap-3 border-t border-base-300 pt-3" bind:this={editor} onsubmit={(event) => { event.preventDefault(); void save(); }}>
				<div class="flex flex-col gap-3">
					<label class="flex max-w-md flex-col gap-1">
						<span class="text-xs text-base-content/70">{t('model-alias-name')}</span>
						<input class="input input-sm font-mono" bind:value={editing.name} placeholder={t('model-alias-name-placeholder')} />
					</label>

					<fieldset class="flex flex-col gap-2">
						<legend class="mb-1 text-xs font-semibold uppercase tracking-wide text-base-content/60">{t('model-alias-targets')}</legend>
						{#each editing.targets as target, index (index)}
							<div class="flex flex-col gap-1.5 border-b border-base-300 pb-2 last:border-b-0 sm:flex-row sm:items-start sm:gap-2 sm:rounded-box sm:border sm:p-2">
								<label class="flex flex-1 flex-col gap-1">
									<span class="text-xs text-base-content/70">{t('model-alias-backend')}</span>
									<select class="select select-sm w-full" bind:value={target.backend} onchange={() => changeBackend(target)}>
										{#each data?.backends ?? [] as backend (backend.name)}
											<option value={backend.name}>{backend.name}</option>
										{/each}
									</select>
								</label>
								<div class="flex flex-1 flex-col gap-1">
									<span class="text-xs text-base-content/70">{t('model-alias-model')}</span>
									<SearchableSelect
										options={modelOptions(target.backend, target.target)}
										value={target.target}
										onchange={(value) => (target.target = value)}
										ariaLabel={t('model-alias-model')}
										size="sm"
										class="w-full"
									/>
								</div>
								<button
									class="btn btn-ghost btn-sm self-start text-error sm:mt-5"
									type="button"
									disabled={editing.targets.length === 1}
									onclick={() => removeTarget(index)}
								>{t('model-alias-remove-target')}</button>
							</div>
							{#if backendModels(target.backend).length === 0}
								<p class="text-xs text-base-content/60">{t('model-alias-no-models')}</p>
							{/if}
						{/each}
						<div>
							<button class="btn btn-xs" type="button" onclick={addTarget} disabled={!data || editing.targets.length >= data.backends.length}>
								+ {t('model-alias-add-target')}
							</button>
						</div>
					</fieldset>

					<div class="flex justify-end gap-2">
						<button class="btn btn-ghost btn-sm" type="button" onclick={() => void cancel()}>{t('upstreams-cancel')}</button>
						<button class="btn btn-primary btn-sm" type="submit" disabled={saving}>{saving ? '…' : t('model-alias-save')}</button>
					</div>
				</div>
			</form>
		{/if}

		{#if data}
			{#if data.aliases.length === 0}
				<div class="rounded-box border border-base-300 p-4 text-sm text-base-content/60">{t('model-alias-empty')}</div>
			{:else}
				<div class="flex flex-col gap-2">
					{#each data.aliases as alias (alias.name)}
						{@const served = servedTargetCount(alias)}
						{@const total = alias.targets.length}
						<div class="rounded-box border border-base-300 px-2 py-2 sm:p-3" data-testid={`model-alias-${alias.name}`}>
							<div class="flex flex-wrap items-center justify-between gap-2">
								<div class="flex flex-wrap items-center gap-2">
									<span class="font-mono text-sm font-semibold">{alias.name}</span>
									<span class="badge badge-xs {served === total ? 'badge-success' : served > 0 ? 'badge-warning' : 'badge-error'}">
										{t('model-alias-served', { served, total })}
									</span>
								</div>
								<div class="flex gap-1">
									<button class="btn btn-ghost btn-xs" type="button" onclick={() => startEdit(alias)}>{t('model-alias-edit')}</button>
									<button class="btn btn-ghost btn-xs text-error" type="button" disabled={removing === alias.name} onclick={() => void remove(alias)}>{t('model-alias-delete')}</button>
								</div>
							</div>
							<ul class="mt-2 flex flex-col gap-1">
								{#each alias.targets as target (target.backend)}
									<li class="flex flex-wrap items-center gap-2 text-xs">
										<span class="font-mono">{target.backend}</span>
										<span class="text-base-content/40" aria-hidden="true">→</span>
										<span class="font-mono">{target.target ?? t('model-alias-sole-model')}</span>
										{#if target.served}
											<span class="badge badge-success badge-xs">{t('model-alias-resolves')}</span>
										{:else}
											<span class="badge badge-warning badge-xs">{t('model-alias-unresolved')}</span>
										{/if}
										{#if target.served && target.resolves_to && target.resolves_to !== target.target}
											<span class="text-base-content/50">{target.resolves_to}</span>
										{/if}
									</li>
								{/each}
							</ul>
						</div>
					{/each}
				</div>
			{/if}
		{:else if !error}
			<div class="flex justify-center py-6"><span class="loading loading-spinner"></span></div>
		{/if}

		{#if data && data.backends.length === 0}
			<div class="alert text-sm"><span>{t('model-alias-no-backends')}</span></div>
		{/if}
	</div>
</article>
