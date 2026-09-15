<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { adminDelete, adminPost, adminPut } from '$lib/admin-client';
	import { t, n } from '$lib/i18n.svelte';
	import SearchableSelect from '$lib/components/SearchableSelect.svelte';
	import { completeAliasLine, parallelismMismatch, parseAliases, splitList, type Backend, type BackendTestResult, type Pool } from '$lib/upstreams';

	interface Props {
		backend?: Backend | null;
		currentPool?: string | null;
		pools: Pool[];
		existingNames: string[];
		onSaved: () => void | Promise<void>;
		onCancel?: () => void;
	}

	let { backend = null, currentPool = null, pools, existingNames, onSaved, onCancel }: Props = $props();
	let name = $state(untrack(() => backend?.name ?? ''));
	let baseUrl = $state(untrack(() => backend?.base_url ?? ''));
	let apiKey = $state('');
	let apiKeyEnv = $state(untrack(() => backend?.api_key_env ?? ''));
	let healthPath = $state(untrack(() => backend?.health_path ?? '/models'));
	let weight = $state(untrack(() => backend?.weight ?? 1));
	let maxInflight = $state(untrack(() => backend?.max_inflight ?? 16));
	let poolName = $state(untrack(() => currentPool ?? ''));
	let models = $state(untrack(() => backend?.models.join(', ') ?? ''));
	let aliases = $state(
		untrack(() => backend?.aliases.map(({ alias, target }) => (target ? `${alias}=${target}` : alias)).join('\n') ?? '')
	);
	let probeModels = $state(untrack(() => backend?.probe_models ?? true));
	let supportsEdit = $state(untrack(() => backend?.supports_edit ?? false));
	let overwrite = $state(untrack(() => backend !== null));
	let deleting = $state(false);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let testing = $state(false);
	let testResult = $state<BackendTestResult | null>(null);
	let aliasesInput: HTMLTextAreaElement;

	// A new backend, or an existing one being renamed onto a name in use.
	let nameTaken = $derived(name.trim() !== backend?.name && existingNames.includes(name.trim()));
	let poolOptions = $derived([{ value: '', label: t('backends-field-pool-none') }, ...pools.map((pool) => ({ value: pool.name, label: pool.name, description: pool.kind }))]);

	async function save() {
		busy = true;
		error = null;
		try {
			// A changed name is a *rename*, not a save under a new name: the
			// name is the primary key, so an ordinary save would leave the
			// original row behind and start a second, empty backend. Move the
			// identity first, then write the fields under it.
			if (backend && name.trim() !== backend.name) {
				await adminPost(`/api/v0/admin/backends/${encodeURIComponent(backend.name)}/rename`, { name: name.trim() });
			}
			await adminPut('/api/v0/admin/backends', {
				name,
				base_url: baseUrl,
				api_key_env: apiKeyEnv,
				api_key: apiKey,
				weight,
				max_inflight: maxInflight,
				health_path: healthPath,
				probe_models: probeModels,
				supports_edit: supportsEdit,
				models: splitList(models),
				aliases: parseAliases(aliases),
				pool: poolName || null,
				overwrite
			});
			await onSaved();
			onCancel?.();
		} catch (err) {
			error = String(err);
		} finally {
			busy = false;
		}
	}

	async function remove() {
		if (!backend) return;
		if (!deleting) {
			deleting = true;
			return;
		}
		busy = true;
		error = null;
		try {
			await adminDelete(`/api/v0/admin/backends/${encodeURIComponent(backend.name)}`);
			await onSaved();
		} catch (err) {
			error = String(err);
		} finally {
			busy = false;
			deleting = false;
		}
	}

	function keySource(result: BackendTestResult) {
		switch (result.key_source?.kind) {
			case 'typed': return t('backends-test-key-typed');
			case 'stored': return t('backends-test-key-stored');
			case 'env': return t('backends-test-key-env', { var: result.key_source.name ?? '' });
			case 'env_unset': return t('backends-test-key-env-unset', { var: result.key_source.name ?? '' });
			default: return t('backends-test-key-none');
		}
	}

	function testMessage(result: BackendTestResult) {
		const source = keySource(result);
		switch (result.code) {
			case 'ok': return t('backends-test-ok', { source, count: result.model_count ?? result.models.length });
			case 'ok_no_models': return t('backends-test-ok-no-models', { source });
			case 'auth_failed': return t('backends-test-auth-failed', { source, status: result.status ?? 401 });
			case 'http_error': return t('backends-test-http-error', { url: result.url ?? baseUrl, status: result.status ?? 500 });
			case 'unreachable': return t('backends-test-unreachable', { url: result.url ?? baseUrl, err: result.detail ?? '' });
			case 'timeout': return t('backends-test-timeout', { url: result.url ?? baseUrl, secs: result.timeout_seconds ?? 8 });
			case 'base_url_required': return t('backends-error-base-url-required');
		}
	}

	async function testConnection() {
		testing = true;
		testResult = null;
		try {
			testResult = await adminPost<BackendTestResult>('/api/v0/admin/backends/test', {
				name,
				base_url: baseUrl,
				api_key_env: apiKeyEnv,
				api_key: apiKey,
				health_path: healthPath
			});
		} catch (err) {
			error = String(err);
		} finally {
			testing = false;
		}
	}

	async function insertModel(model: string) {
		const completed = completeAliasLine(aliases, aliasesInput.selectionStart ?? aliases.length, model);
		aliases = completed.value;
		await tick();
		aliasesInput.setSelectionRange(completed.cursor, completed.cursor);
		aliasesInput.focus();
	}
</script>

<form class="flex flex-col gap-3" onsubmit={(event) => { event.preventDefault(); void save(); }}>
	{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}
	{#if nameTaken}
		<div class="alert alert-warning text-sm" role="alert">
			<span>{t('backends-name-taken')}</span>
			<label class="label gap-2">
				<input class="checkbox checkbox-sm" type="checkbox" bind:checked={overwrite} />
				<span>{t('admin-overwrite-existing')}</span>
			</label>
		</div>
	{/if}
	<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('backends-field-name')}</span>
			<input class="input input-bordered input-sm font-mono w-full" bind:value={name} required />
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('backends-field-base-url')}</span>
			<input class="input input-bordered input-sm font-mono w-full" bind:value={baseUrl} required />
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('backends-field-api-key')}</span>
			<input class="input input-bordered input-sm font-mono w-full" type="password" autocomplete="off" bind:value={apiKey} placeholder={backend?.has_stored_key ? t('backends-field-api-key-keep') : t('backends-field-api-key-placeholder')} />
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('backends-field-api-key-env')}</span>
			<input class="input input-bordered input-sm font-mono w-full" bind:value={apiKeyEnv} />
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('backends-field-health-path')}</span>
			<input class="input input-bordered input-sm font-mono w-full" bind:value={healthPath} />
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('backends-field-pool')}</span>
			<SearchableSelect options={poolOptions} bind:value={poolName} ariaLabel={t('backends-field-pool')} size="sm" class="w-full" />
			<span class="text-xs text-base-content/50">{t('backends-field-pool-hint')}</span>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('backends-field-weight')}</span>
			<input class="input input-bordered input-sm w-full" type="number" min="1" bind:value={weight} />
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('backends-field-max-inflight')}</span>
			<input class="input input-bordered input-sm w-full" type="number" min="1" bind:value={maxInflight} />
		</label>
	</div>
	<label class="flex flex-col gap-1">
		<span class="text-xs text-base-content/70">{t('backends-field-models')}</span>
		<input class="input input-bordered input-sm font-mono w-full" bind:value={models} />
	</label>
	<label class="flex flex-col gap-1">
		<span class="text-xs text-base-content/70">{t('backends-field-aliases')}</span>
		<textarea bind:this={aliasesInput} class="textarea textarea-bordered textarea-sm font-mono w-full" rows="3" bind:value={aliases}></textarea>
	</label>
	<div class="flex flex-wrap gap-4">
		<label class="label gap-2"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={probeModels} /><span>{t('backends-field-probe-models')}</span></label>
		<label class="label gap-2"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={supportsEdit} /><span>{t('backends-field-supports-edit')}</span></label>
	</div>
	<div class="flex flex-col gap-2">
		<div class="flex flex-wrap items-center gap-2">
			<button class="btn btn-outline btn-sm" type="button" disabled={testing || !baseUrl.trim()} onclick={() => void testConnection()}>{t('backends-test-button')}</button>
			<span class="text-xs text-base-content/50">{t('backends-test-hint')}</span>
		</div>
		{#if testResult}
			<div class={`alert text-sm ${testResult.outcome === 'success' ? 'alert-success' : testResult.outcome === 'warning' ? 'alert-info' : 'alert-error'}`} role="status"><span>{testMessage(testResult)}</span></div>
			{#if testResult.profile && testResult.profile !== 'generic'}
				<!--
					What the server turned out to be, and the two things that
					follow from it. Shown here because this is where an operator
					can still act on them — a context window nobody reports is a
					value they have to look up, and a server that runs one
					request at a time is a max-inflight they should lower.
				-->
				{@const parallelWarning = parallelismMismatch(testResult.detected_max_parallel, maxInflight)}
				<div class="flex flex-wrap items-center gap-1 text-xs">
					<span class="badge badge-ghost badge-sm font-mono" title={testResult.detected_version ?? undefined}>{t('backends-detect-profile', { profile: testResult.profile })}</span>
					{#if parallelWarning !== null}
						<span class="badge badge-warning badge-sm">{t('backends-parallel-mismatch', { parallel: parallelWarning })}</span>
					{/if}
					<span class="text-base-content/60">
						{testResult.detected_context ? t('admin-context-detected', { window: n(testResult.detected_context) }) : t('admin-context-unreported')}
					</span>
				</div>
			{/if}
			{#if testResult.models.length}
				<div class="flex flex-col gap-1">
					<span class="text-xs text-base-content/60">{t('backends-test-insert-hint')}</span>
					<div class="flex flex-wrap gap-1">
						{#each testResult.models as model (model)}<button class="badge badge-outline badge-sm cursor-pointer font-mono" type="button" onclick={() => void insertModel(model)}>{model}</button>{/each}
					</div>
				</div>
			{/if}
		{/if}
	</div>
	<div class="flex items-center gap-2">
		{#if backend}
			<button class="btn btn-ghost btn-sm text-error" type="button" disabled={busy} onclick={() => void remove()}>
				{deleting ? t('upstreams-delete-confirm') : t('backends-delete-backend')}
			</button>
		{/if}
		<span class="flex-1"></span>
		{#if onCancel}<button class="btn btn-ghost btn-sm" type="button" onclick={onCancel}>{t('upstreams-cancel')}</button>{/if}
		<button class="btn btn-primary btn-sm" type="submit" disabled={busy || !name.trim() || !baseUrl.trim() || (nameTaken && !overwrite)}>
			{t('backends-save-backend')}
		</button>
	</div>
</form>
