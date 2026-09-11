<script lang="ts">
	import { adminPatch, adminPost } from '$lib/admin-client';
	import { untrack } from 'svelte';
	import { t } from '$lib/i18n.svelte';
	import SearchableSelect from '$lib/components/SearchableSelect.svelte';
	import { parseList, type RagCollection, type RagProfile, type RagProvider } from '$lib/rag';

	let {
		collection = null,
		providers,
		profiles,
		models,
		defaultModel = null,
		onsaved,
		oncancel
	} = $props<{
		collection?: RagCollection | null;
		providers: RagProvider[];
		profiles: RagProfile[];
		models: string[];
		defaultModel?: string | null;
		onsaved: (name: string, aggregate: boolean) => void | Promise<void>;
		oncancel?: () => void;
	}>();

	const initialCollection = untrack(() => collection);
	const profile = untrack(() => profiles.find((item: RagProfile) => item.id === initialCollection?.profile_id));
	let form = $state({
		name: initialCollection?.name ?? '',
		description: initialCollection?.description ?? '',
		git_url: initialCollection?.git_url ?? '',
		git_ref: initialCollection?.git_ref ?? 'main',
		pat: '',
		clear_pat: false,
		source_kind: initialCollection?.source_kind ?? 'git',
		profile: profile?.name ?? '',
		extraction_model: initialCollection?.extraction_model ?? '',
		embedding_model: initialCollection?.embedding_model ?? '',
		include_globs: initialCollection?.include_globs.join(', ') ?? '',
		exclude_globs: initialCollection?.exclude_globs.join(', ') ?? '',
		chunk_size: initialCollection?.chunk_size ?? 800,
		chunk_overlap: initialCollection?.chunk_overlap ?? 100,
		aggregate: initialCollection?.search_mode === 'aggregate',
		allowed_groups: initialCollection?.allowed_groups.join(', ') ?? ''
	});
	let sourceConfig = $state<Record<string, string>>({ ...(initialCollection?.source_config ?? {}) });
	let busy = $state(false);
	let testing = $state(false);
	let message = $state<string | null>(null);
	const selectedProvider = $derived(providers.find((item: RagProvider) => item.kind === form.source_kind));
	let providerOptions = $derived(providers.map((provider: RagProvider) => ({ value: provider.kind, label: provider.label, description: provider.description })));
	let embeddingOptions = $derived([
		...(!form.embedding_model ? [{ value: '', label: t('rag-option-choose-embedding-model'), disabled: true }] : []),
		...(form.embedding_model && !models.includes(form.embedding_model) ? [{ value: form.embedding_model, label: `${form.embedding_model} ${t('rag-suffix-not-advertised')}` }] : []),
		...models.map((model: string) => ({ value: model, label: model }))
	]);
	let profileOptions = $derived([
		{ value: '', label: t('rag-option-profile-none') },
		...profiles.map((item: RagProfile) => ({ value: item.name, label: item.name, description: item.description || undefined }))
	]);
	$effect(() => {
		if (!collection && !form.embedding_model && defaultModel) form.embedding_model = defaultModel;
	});

	async function save() {
		busy = true;
		message = null;
		try {
			const body: Record<string, unknown> = {
				description: form.description.trim() || null,
				git_url: form.git_url.trim(),
				git_ref: form.git_ref.trim() || 'main',
				source_kind: form.source_kind,
				source_config: sourceConfig,
				profile: form.profile || null,
				extraction_model: form.extraction_model.trim() || null,
				embedding_model: form.embedding_model.trim(),
				include_globs: parseList(form.include_globs),
				exclude_globs: parseList(form.exclude_globs),
				chunk_size: Number(form.chunk_size),
				chunk_overlap: Number(form.chunk_overlap),
				search_mode: form.aggregate ? 'aggregate' : 'versioned',
				allowed_groups: parseList(form.allowed_groups)
			};
			if (collection) {
				if (form.clear_pat) body.pat = null;
				else if (form.pat) body.pat = form.pat;
				await adminPatch(`/api/v0/rag/collections/${collection.id}`, body);
			} else {
				body.name = form.name.trim();
				body.pat = form.pat || null;
				await adminPost('/api/v0/rag/collections', body);
			}
			await onsaved(collection?.name ?? form.name, form.aggregate);
			if (!collection) {
				form.name = '';
				form.description = '';
				form.git_url = '';
				form.pat = '';
			}
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function testSource() {
		testing = true;
		message = null;
		try {
			if (form.source_kind === 'git') {
				message = t('rag-source-test-git');
				return;
			}
			const result = await adminPost<{
				ok: boolean;
				account?: string;
				root_entries?: number;
				server?: string;
				error?: string;
			}>('/api/v0/rag/test-source', {
				source_kind: form.source_kind,
				source_config: sourceConfig
			});
			if (!result.ok) message = t('rag-source-test-failed', { error: result.error ?? '' });
			else {
				message = result.account
					? t('rag-source-test-ok', {
							account: result.account,
							entries: result.root_entries ?? 0
						})
					: t('rag-source-test-ok-plain', { entries: result.root_entries ?? 0 });
				if (result.server) message += ` ${t('rag-source-detected', { server: result.server })}`;
			}
		} catch (error) {
			message = String(error);
		} finally {
			testing = false;
		}
	}
</script>

<div class="card card-border min-w-0 bg-base-100">
	<div class="card-body min-w-0 gap-4">
		<div>
			<h2 class="card-title">{collection ? t('rag-edit-heading', { name: collection.name }) : t('rag-create-heading')}</h2>
			{#if !collection}<p class="mt-1 text-sm text-base-content/60">{t('rag-create-description')}</p>{/if}
		</div>

		{#if message}<div class="alert alert-info"><span>{message}</span></div>{/if}

		<div class="grid min-w-0 grid-cols-1 gap-3 [&>.fieldset]:min-w-0 [&_input]:min-w-0 [&_textarea]:min-w-0 md:grid-cols-2">
			{#if !collection}
				<fieldset class="fieldset">
					<legend class="fieldset-legend">{t('rag-label-name')}</legend>
					<input class="input w-full" bind:value={form.name} placeholder={t('rag-placeholder-name')} />
				</fieldset>
			{/if}
			<fieldset class="fieldset {collection ? 'md:col-span-2' : ''}">
				<legend class="fieldset-legend">{collection ? t('rag-label-description') : t('rag-label-description-optional')}</legend>
				<input class="input w-full" bind:value={form.description} placeholder={t('rag-placeholder-description')} />
			</fieldset>

			<fieldset class="fieldset">
				<legend class="fieldset-legend">{t('rag-label-source-kind')}</legend>
				<SearchableSelect options={providerOptions} bind:value={form.source_kind} ariaLabel={t('rag-label-source-kind')} class="w-full" disabled={collection?.search_mode === 'aggregate'} />
				{#if selectedProvider}<p class="label max-w-full whitespace-normal">{selectedProvider.kind === 'git' ? t('rag-source-git-help') : selectedProvider.description}</p>{/if}
			</fieldset>
			<fieldset class="fieldset">
				<legend class="fieldset-legend">{t('rag-label-embedding-model')}</legend>
				{#if models.length}
					<SearchableSelect options={embeddingOptions} bind:value={form.embedding_model} ariaLabel={t('rag-label-embedding-model')} class="w-full" />
				{:else}
					<input class="input w-full" bind:value={form.embedding_model} placeholder={t('rag-placeholder-embedding-model-none')} />
				{/if}
			</fieldset>

			{#if form.source_kind === 'git'}
				<fieldset class="fieldset">
					<legend class="fieldset-legend">{t('rag-label-git-url-versioned')}</legend>
					<input class="input w-full font-mono" bind:value={form.git_url} placeholder={t('rag-placeholder-git-url')} />
				</fieldset>
				<fieldset class="fieldset">
					<legend class="fieldset-legend">{t('rag-label-branch-tag')}</legend>
					<input class="input w-full font-mono" bind:value={form.git_ref} placeholder={t('rag-placeholder-branch-tag-commit')} />
				</fieldset>
				<fieldset class="fieldset md:col-span-2">
					<legend class="fieldset-legend">{collection ? t('rag-label-pat') : t('rag-label-pat-optional')}</legend>
					<input class="input w-full" type="password" bind:value={form.pat} placeholder={collection ? t('rag-placeholder-pat-keep') : t('rag-placeholder-pat')} />
					{#if collection?.pat_set}
						<label class="label cursor-pointer justify-start gap-2"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={form.clear_pat} /> {t('rag-label-clear-pat')}</label>
					{/if}
				</fieldset>
			{:else if selectedProvider}
				{#each selectedProvider.fields as field (field.key)}
					<fieldset class="fieldset">
						<legend class="fieldset-legend">{field.label}{field.required ? ' *' : ''}</legend>
						<input class="input w-full" type={field.secret ? 'password' : 'text'} value={sourceConfig[field.key] ?? ''} placeholder={field.secret && collection?.source_secrets_set ? t('rag-source-secret-placeholder') : (field.default ?? '')} oninput={(event) => (sourceConfig[field.key] = event.currentTarget.value)} />
						{#if field.help}<p class="label max-w-full whitespace-normal">{field.help}</p>{/if}
					</fieldset>
				{/each}
			{/if}

			<fieldset class="fieldset">
				<legend class="fieldset-legend">{t('rag-label-profile')}</legend>
				<SearchableSelect options={profileOptions} bind:value={form.profile} ariaLabel={t('rag-label-profile')} class="w-full" />
				<p class="label max-w-full whitespace-normal">{t('rag-profile-help')}</p>
			</fieldset>

			<fieldset class="fieldset">
				<legend class="fieldset-legend">{t('rag-label-include-globs-full')}</legend>
				<textarea class="textarea w-full font-mono" rows="2" bind:value={form.include_globs} placeholder={t('rag-placeholder-include-globs')}></textarea>
			</fieldset>
			<fieldset class="fieldset">
				<legend class="fieldset-legend">{t('rag-label-exclude-globs')}</legend>
				<textarea class="textarea w-full font-mono" rows="2" bind:value={form.exclude_globs} placeholder={t('rag-placeholder-exclude-globs')}></textarea>
			</fieldset>
			<fieldset class="fieldset">
				<legend class="fieldset-legend">{t('rag-label-chunk-size')}</legend>
				<input class="input w-full" type="number" min="1" max="8000" bind:value={form.chunk_size} />
			</fieldset>
			<fieldset class="fieldset">
				<legend class="fieldset-legend">{t('rag-label-chunk-overlap')}</legend>
				<input class="input w-full" type="number" min="0" bind:value={form.chunk_overlap} />
			</fieldset>
			{#if collection}
				<fieldset class="fieldset md:col-span-2">
					<legend class="fieldset-legend">{t('rag-label-allowed-groups')}</legend>
					<input class="input w-full" bind:value={form.allowed_groups} />
					<p class="label max-w-full whitespace-normal">{t('rag-hint-allowed-groups')}</p>
				</fieldset>
			{/if}
		</div>

		{#if !collection}
			<label class="label min-w-0 cursor-pointer justify-start gap-3 whitespace-normal">
				<input class="checkbox" type="checkbox" bind:checked={form.aggregate} />
				<span class="min-w-0">{t('rag-create-aggregate-help')}</span>
			</label>
		{/if}
		{#if selectedProvider?.auth.kind === 'oauth2'}<p class="text-sm text-base-content/60">{t('rag-source-consent-save-first')}</p>{/if}

		<div class="card-actions justify-end">
			{#if oncancel}<button class="btn" type="button" onclick={oncancel}>{t('rag-button-cancel')}</button>{/if}
			{#if form.source_kind !== 'git'}<button class="btn" type="button" onclick={testSource} disabled={testing}>{testing ? t('rag-source-testing') : t('rag-source-test-button')}</button>{/if}
			<button class="btn btn-primary" type="button" onclick={save} disabled={busy || (!collection && !form.name.trim()) || !form.embedding_model.trim()}>{collection ? t('rag-button-save-changes') : t('rag-button-queue-indexing')}</button>
		</div>
	</div>
</div>
