<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPut, adminDelete } from '$lib/admin-client';
	import { t, n } from '$lib/i18n.svelte';

	interface ModelEntry {
		name: string;
		configured: boolean;
		defaults:
			| {
					defaults_toml: string;
					reasoning_style: string | null;
					context_window: number | null;
					input_price: number | null;
					output_price: number | null;
					pricing_unit: string;
					budget_standard: number | null;
					budget_deep: number | null;
					budget_max: number | null;
					effort_standard: string | null;
					effort_deep: string | null;
					effort_max: string | null;
					capabilities: Record<string, unknown>;
			  }
			| null;
	}
	interface ModelsData {
		models: ModelEntry[];
		currency: string;
		feature_defaults: { feature: string; model: string | null }[];
		search: { provider: string; searxng_url: string | null; brave_key_set: boolean };
	}

	let data = $state<ModelsData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let editing = $state<string | null>(null);

	// Editor state.
	let fname = $state('');
	let finput = $state('');
	let foutput = $state('');
	let funit = $state('per_mtok');
	let fcontext = $state('');
	let ftoml = $state('');

	async function refresh() {
		try {
			data = await adminJson<ModelsData>('/api/v0/admin/models');
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	function openEdit(m: ModelEntry) {
		editing = m.name;
		fname = m.name;
		const d = m.defaults;
		finput = d?.input_price?.toString() ?? '';
		foutput = d?.output_price?.toString() ?? '';
		funit = d?.pricing_unit ?? 'per_mtok';
		fcontext = d?.context_window?.toString() ?? '';
		ftoml = d?.defaults_toml ?? '';
	}

	async function save() {
		notice = null;
		try {
			await adminPut('/api/v0/admin/models', {
				model_name: fname,
				input_price: finput,
				output_price: foutput,
				pricing_unit: funit,
				context_window: fcontext,
				defaults_toml: ftoml
			});
			editing = null;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function clear(name: string) {
		if (!confirm(t('admin-clear-overrides-confirm', { model: name }))) return;
		try {
			await adminDelete(`/api/v0/admin/models/${encodeURIComponent(name)}`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function saveFeatureDefault(feature: string, model: string) {
		try {
			await adminPut('/api/v0/admin/models/defaults', { feature, model });
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	let searchProvider = $state('');
	let searchUrl = $state('');
	let searchKey = $state('');

	async function saveSearch() {
		try {
			await adminPut('/api/v0/admin/models/search', {
				provider: searchProvider,
				searxng_url: searchUrl,
				brave_api_key: searchKey,
				clear_brave_key: false
			});
			searchKey = '';
			notice = t('admin-search-saved');
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	$effect(() => {
		if (data && !searchProvider) {
			searchProvider = data.search.provider;
			searchUrl = data.search.searxng_url ?? '';
		}
	});

	onMount(refresh);
</script>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

{#if data}
	<div class="card border border-base-300 mb-6">
		<div class="card-body">
			<h2 class="card-title text-base">{t('admin-defaults-heading')}</h2>
			<div class="grid sm:grid-cols-2 gap-3">
				{#each data.feature_defaults as fd (fd.feature)}
					<form
						class="join"
						onsubmit={(e) => {
							e.preventDefault();
							const input = e.currentTarget.querySelector('input') as HTMLInputElement;
							void saveFeatureDefault(fd.feature, input.value);
						}}
					>
						<span class="badge badge-outline badge-sm self-center me-2">{fd.feature}</span>
						<input
							class="input input-bordered input-sm join-item w-full"
							placeholder={fd.model ?? t('backends-field-pool-none')}
							aria-label={t('admin-defaults-model-aria', { feature: fd.feature })}
						/>
						<button class="btn btn-outline btn-sm join-item" type="submit">{t('admin-defaults-set')}</button>
					</form>
				{/each}
			</div>
		</div>
	</div>

	<div class="card border border-base-300 mb-6">
		<div class="card-body">
			<h2 class="card-title text-base">{t('admin-search-heading')}</h2>
			<div class="flex flex-wrap gap-3 items-end">
				<label class="flex flex-col gap-1 w-40">
					<span class="label-text">{t('admin-search-provider-label')}</span>
					<select class="select select-bordered select-sm" bind:value={searchProvider}>
						<option value="searxng">{t('admin-search-provider-searxng')}</option>
						<option value="brave">{t('admin-search-provider-brave')}</option>
						<option value="none">{t('admin-search-provider-none')}</option>
					</select>
				</label>
				<label class="flex flex-col gap-1 flex-1 min-w-48">
					<span class="label-text">{t('admin-search-searxng-url-label')}</span>
					<input
						class="input input-bordered input-sm"
						bind:value={searchUrl}
						placeholder={t('admin-search-searxng-url-placeholder')}
					/>
				</label>
				<label class="flex flex-col gap-1 flex-1 min-w-48">
					<span class="label-text">{t('admin-search-brave-key-label')}</span>
					<input
						class="input input-bordered input-sm"
						type="password"
						bind:value={searchKey}
						placeholder={data.search.brave_key_set ? t('admin-search-brave-key-placeholder') : ''}
					/>
				</label>
				<button class="btn btn-primary btn-sm" onclick={saveSearch}>{t('admin-search-save')}</button>
			</div>
		</div>
	</div>

	{#if editing !== null}
		<div class="card border border-base-300 mb-6">
			<div class="card-body">
				<h2 class="card-title text-base">
					{editing === ''
						? t('admin-add-overrides-heading')
						: t('admin-edit-model-heading', { model: editing })}
				</h2>
				<div class="grid sm:grid-cols-2 gap-3">
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('admin-col-model')}</span>
						<input class="input input-bordered" bind:value={fname} />
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('admin-context-window-full-label')}</span>
						<input
							class="input input-bordered"
							bind:value={fcontext}
							placeholder={t('admin-context-window-placeholder')}
						/>
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('admin-price-in-label')} ({data.currency})</span>
						<input
							class="input input-bordered"
							bind:value={finput}
							placeholder={t('admin-price-in-placeholder')}
						/>
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('admin-price-out-label')} ({data.currency})</span>
						<input
							class="input input-bordered"
							bind:value={foutput}
							placeholder={t('admin-price-out-placeholder')}
						/>
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('admin-pricing-unit-label')}</span>
						<select class="select select-bordered" bind:value={funit}>
							<option value="per_mtok">{t('admin-pricing-unit-mtok')}</option>
							<option value="per_ktok">{t('admin-pricing-unit-ktok')}</option>
							<option value="per_1k_imgs">{t('admin-pricing-unit-kimgs')}</option>
						</select>
					</label>
					<label class="flex flex-col gap-1 sm:col-span-2">
						<span class="label-text">{t('admin-toml-defaults-label')}</span>
						<textarea class="textarea textarea-bordered font-mono text-xs" rows="4" bind:value={ftoml}></textarea>
					</label>
				</div>
				<div class="card-actions justify-end mt-2">
					<button class="btn btn-ghost btn-sm" onclick={() => (editing = null)}>{t('admin-cancel')}</button>
					<button class="btn btn-primary btn-sm" onclick={save} disabled={!fname.trim()}>
						{t('admin-save-model')}
					</button>
				</div>
			</div>
		</div>
	{/if}

	<div class="card border border-base-300">
		<div class="card-body">
			<div class="flex justify-between items-center mb-2">
				<h2 class="card-title text-base">{t('admin-heading')}</h2>
				<button class="btn btn-ghost btn-sm" onclick={() => { editing = ''; fname = ''; finput = ''; foutput = ''; fcontext = ''; ftoml = ''; }}>
					{t('admin-add-model')}
				</button>
			</div>
			<ul class="flex flex-col divide-y divide-base-300">
				{#each data.models as m (m.name)}
					<li class="py-2 flex items-center gap-3 flex-wrap">
						<span class="font-mono text-sm">{m.name}</span>
						{#if m.defaults?.input_price != null || m.defaults?.output_price != null}
							<span class="badge badge-ghost badge-sm">
								{m.defaults?.input_price != null ? n(m.defaults.input_price) : '–'} /
								{m.defaults?.output_price != null ? n(m.defaults.output_price) : '–'}
								{data.currency}
							</span>
						{/if}
						{#if m.defaults?.context_window != null}
							<span class="badge badge-ghost badge-sm">
								{n(m.defaults.context_window)} {t('admin-badge-ctx')}
							</span>
						{/if}
						{#if !m.configured}
							<span class="text-xs text-base-content/50">{t('admin-not-configured')}</span>
						{/if}
						<span class="flex-1"></span>
						<button class="btn btn-ghost btn-sm" onclick={() => openEdit(m)}>{t('rag-button-edit')}</button>
						{#if m.configured}
							<button class="btn btn-ghost btn-sm text-error" onclick={() => clear(m.name)}>
								{t('settings-secret-clear')}
							</button>
						{/if}
					</li>
				{/each}
			</ul>
		</div>
	</div>
{/if}
