<script lang="ts">
	import type { BackendPriority } from '#lib/priority.js';
	import { tick, untrack } from 'svelte';
	import { adminDelete, adminPost, adminPut } from '#lib/admin-client.js';
	import { ApiError, request } from '#lib/api.js';
	import { t, n } from '#lib/i18n.svelte.js';
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import { backendTestFailure, completeAliasLine, formatHeaders, parallelismMismatch, parseAliases, parseHeaders, splitList, type Backend, type BackendTestResult, type Pool } from '#lib/upstreams.js';

	interface Props {
		backend?: Backend | null;
		currentPool?: string | null;
		pools: Pool[];
		poolKinds: string[];
		existingNames: string[];
		onSaved: () => void | Promise<void>;
		onCancel?: () => void;
	}

	let { backend = null, currentPool = null, pools, poolKinds, existingNames, onSaved, onCancel }: Props = $props();
	let name = $state(untrack(() => backend?.name ?? ''));
	let provider = $state<'openai_compatible' | 'openrouter'>(untrack(() => backend?.provider ?? 'openai_compatible'));
	let baseUrl = $state(untrack(() => backend?.base_url ?? ''));
	let apiKey = $state('');
	let apiKeyEnv = $state(untrack(() => backend?.api_key_env ?? ''));
	let healthPath = $state(untrack(() => backend?.health_path ?? '/models'));
	let weight = $state(untrack(() => backend?.weight ?? 1));
	let maxInflight = $state(untrack(() => backend?.max_inflight ?? 16));
	let poolName = $state(untrack(() => currentPool ?? ''));
	// OpenRouter serves one pool per kind: per kind, join an existing empty
	// pool of that kind, create openrouter-<kind>, or not serve the kind.
	let kindSelection = $state<Record<string, string>>(
		untrack(() => {
			const selection: Record<string, string> = {};
			for (const pool of pools ?? []) {
				if (backend && pool.backends.includes(backend.name)) selection[pool.kind] = pool.name;
			}
			if (currentPool && !backend) {
				const pool = (pools ?? []).find((candidate) => candidate.name === currentPool);
				if (pool) selection[pool.kind] = pool.name;
			}
			return selection;
		})
	);
	let openRouterKinds = $derived(poolKinds.filter((kind) => kind !== 'ocr'));
	/** Existing pools of a kind that hold no other backend — safe to adopt. */
	function adoptablePools(kind: string) {
		return (pools ?? []).filter((pool) => pool.kind === kind && pool.backends.length === 0);
	}
	let models = $state(untrack(() => backend?.models.join(', ') ?? ''));
	let aliases = $state(
		untrack(() => backend?.aliases.map(({ alias, target }) => (target ? `${alias}=${target}` : alias)).join('\n') ?? '')
	);
	let extraHeaders = $state(untrack(() => formatHeaders(backend?.extra_headers ?? {})));
	let supportsEdit = $state(untrack(() => backend?.supports_edit ?? false));
	let priorityMode = $state<BackendPriority['mode']>(untrack(() => backend?.priority.mode ?? 'auto'));
	let priorityField = $state(untrack(() => backend?.priority.mode === 'custom' ? backend.priority.mapping.field : 'priority'));
	let priorityLow = $state(untrack(() => backend?.priority.mode === 'custom' ? backend.priority.mapping.low : 1));
	let priorityMedium = $state(untrack(() => backend?.priority.mode === 'custom' ? backend.priority.mapping.medium : 0));
	let priorityHigh = $state(untrack(() => backend?.priority.mode === 'custom' ? backend.priority.mapping.high : -1));
	let overwrite = $state(untrack(() => backend !== null));
	let advancedOpen = $state(false);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let errorAlert: HTMLDivElement | undefined = $state();
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
				provider,
				base_url: baseUrl,
				api_key_env: apiKeyEnv,
				api_key: apiKey,
				priority: priorityMode === 'custom' ? { mode: priorityMode, mapping: { field: priorityField, low: priorityLow, medium: priorityMedium, high: priorityHigh } } : { mode: priorityMode },
				weight,
				max_inflight: maxInflight,
				health_path: healthPath,
				supports_edit: supportsEdit,
				models: splitList(models),
				aliases: parseAliases(aliases),
				extra_headers: parseHeaders(extraHeaders),
				pools: provider === 'openrouter'
					? Object.values(kindSelection).filter((value) => value !== '' && value !== 'new')
					: poolName ? [poolName] : [],
				new_pools: provider === 'openrouter'
					? openRouterKinds.filter((kind) => kindSelection[kind] === 'new').map((kind) => ({ name: `openrouter-${kind}`, kind }))
					: [],
				overwrite
			});
			await onSaved();
			onCancel?.();
		} catch (err) {
			error = String(err);
			await tick();
			errorAlert?.focus();
		} finally {
			busy = false;
		}
	}

	async function remove() {
		if (!backend) return;
		if (!confirm(t('backends-delete-confirm', { name: backend.name }))) return;
		busy = true;
		error = null;
		try {
			await adminDelete(`/api/v0/admin/backends/${encodeURIComponent(backend.name)}`);
			await onSaved();
			onCancel?.();
		} catch (err) {
			error = String(err);
		} finally {
			busy = false;
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
			case 'auth_failed': return withUpstreamDetail(t('backends-test-auth-failed', { source, status: result.status ?? 401 }), result.detail);
			case 'http_error': return withUpstreamDetail(t('backends-test-http-error', { url: result.url ?? baseUrl, status: result.status ?? 500 }), result.detail);
			case 'unreachable': return t('backends-test-unreachable', { url: result.url ?? baseUrl, err: result.detail ?? '' });
			case 'timeout': return t('backends-test-timeout', { url: result.url ?? baseUrl, secs: result.timeout_seconds ?? 8 });
			case 'base_url_required': return t('backends-error-base-url-required');
		}
	}

	// The backend's own response body is the detail of a refused test — it says
	// what was actually wrong (a missing anthropic-version, a wrong path).
	function withUpstreamDetail(message: string, detail?: string) {
		return detail ? `${message} ${t('backends-test-upstream-said', { detail })}` : message;
	}

	async function testConnection() {
		testing = true;
		testResult = null;
		try {
			testResult = await request<BackendTestResult>('/api/v0/admin/backends/test', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ name, provider, base_url: baseUrl, api_key_env: apiKeyEnv, api_key: apiKey, health_path: healthPath, extra_headers: parseHeaders(extraHeaders) })
			});
		} catch (err) {
			const failed = err instanceof ApiError ? backendTestFailure(err.detail) : null;
			if (failed) testResult = failed;
			else error = err instanceof ApiError ? (err.serverMessage ?? err.message) : String(err);
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
	{#if error}<div class="alert alert-error text-sm" role="alert" tabindex="-1" bind:this={errorAlert}><span>{error}</span></div>{/if}
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
			<input class="input font-mono w-full" bind:value={name} required />
		</label>
		{#if backend}
			<label class="flex flex-col gap-1">
				<span class="text-xs text-base-content/70">{t('openrouter-provider-field')}</span>
				<select
					class="select w-full"
					aria-label={t('openrouter-provider-field')}
					bind:value={provider}
					onchange={() => {
						if (provider === 'openrouter' && !baseUrl.trim()) baseUrl = 'https://openrouter.ai/api/v1';
					}}
				>
					<option value="openai_compatible">{t('openrouter-provider-openai')}</option>
					<option value="openrouter">{t('openrouter-provider-openrouter')}</option>
				</select>
				<span class="text-xs text-base-content/50">{t('openrouter-provider-hint')}</span>
			</label>
		{/if}
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('backends-field-base-url')}</span>
			<input class="input font-mono w-full" bind:value={baseUrl} required />
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-xs text-base-content/70">{t('backends-field-api-key')}</span>
			<input class="input font-mono w-full" type="password" autocomplete="off" bind:value={apiKey} placeholder={backend?.has_stored_key ? t('backends-field-api-key-keep') : t('backends-field-api-key-placeholder')} />
			{#if backend?.api_key_hint}<span class="text-xs text-base-content/50">{t('backends-field-api-key-stored', { hint: backend.api_key_hint })}</span>{/if}
		</label>
		{#if provider !== 'openrouter'}
			<label class="flex flex-col gap-1">
				<span class="text-xs text-base-content/70">{t('backends-field-pool')}</span>
				<SearchableSelect options={poolOptions} bind:value={poolName} ariaLabel={t('backends-field-pool')} size="sm" class="w-full" />
				<span class="text-xs text-base-content/50">{t('backends-field-pool-hint')}</span>
			</label>
		{/if}
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
						{testResult.detected_context_window ? t('admin-context-detected', { window: n(testResult.detected_context_window) }) : t('admin-context-unreported')}
					</span>
				</div>
			{/if}
			{#if testResult.models.length}
				<div class="flex flex-col gap-1">
					<span class="text-xs text-base-content/60">{t('backends-test-insert-hint')}</span>
					<div class="flex flex-wrap gap-1">
						{#each testResult.models as model (model)}<button class="badge badge-outline badge-sm cursor-pointer font-mono" type="button" onclick={() => { advancedOpen = true; void insertModel(model); }}>{model}</button>{/each}
					</div>
				</div>
			{/if}
		{/if}
	</div>
	<details class="advanced" bind:open={advancedOpen}>
		<summary>{t('backends-advanced')}</summary>
		<div class="flex flex-col gap-3 pt-3">
			<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
				<label class="flex flex-col gap-1">
					<span class="text-xs text-base-content/70">{t('backends-field-api-key-env')}</span>
					<input class="input font-mono w-full" bind:value={apiKeyEnv} />
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs text-base-content/70">{t('backends-field-health-path')}</span>
					<input class="input font-mono w-full" bind:value={healthPath} />
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs text-base-content/70">{t('backends-field-weight')}</span>
					<input class="input w-full" type="number" min="1" bind:value={weight} />
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs text-base-content/70">{t('backends-field-max-inflight')}</span>
					<input class="input w-full" type="number" min="1" bind:value={maxInflight} />
				</label>
			</div>
			{#if provider === 'openrouter'}
				<div class="flex flex-col gap-2">
					<span class="text-xs text-base-content/70">{t('backends-field-pools')}</span>
					<div class="flex flex-col gap-1.5">
						{#each openRouterKinds as kind (kind)}
							<div class="flex items-center gap-3">
								<span class="w-36 flex-none text-sm">{kind}</span>
								<select
									class="select select-sm w-full"
									aria-label={kind}
									value={kindSelection[kind] ?? ''}
									onchange={(event) => kindSelection = { ...kindSelection, [kind]: event.currentTarget.value }}
								>
									<option value="">{t('backends-pool-not-served')}</option>
									<option value="new">+ {t('backends-pool-new', { kind })}</option>
									{#each adoptablePools(kind) as pool (pool.name)}
										<option value={pool.name}>{pool.name}</option>
									{/each}
								</select>
							</div>
						{/each}
					</div>
					<span class="text-xs text-base-content/50">{t('backends-field-pools-hint')}</span>
				</div>
			{/if}
			<fieldset class="fieldset">
				<legend class="fieldset-legend">{t('priority-backend-label')}</legend>
				<select aria-label={t('priority-backend-label')} class="select w-full" bind:value={priorityMode}>
					<option value="auto">{t('priority-mode-auto')}</option>
					<option value="gateway">{t('priority-mode-gateway')}</option>
					<option value="custom">{t('priority-mode-custom')}</option>
				</select>
				<p class="text-xs text-base-content/60">{t('priority-backend-help')}</p>
				{#if priorityMode === 'auto'}<p class="text-xs text-base-content/60">{backend?.live?.detected_priority ? t('priority-detected') : t('priority-unreported')}</p>{/if}
				{#if priorityMode === 'custom'}
					<label class="fieldset"><span class="fieldset-legend">{t('priority-field')}</span><input class="input w-full font-mono" bind:value={priorityField} required /></label>
					<div class="grid grid-cols-3 gap-2">
						<label class="fieldset"><span class="fieldset-legend">{t('priority-low')}</span><input class="input w-full" type="number" step="1" bind:value={priorityLow} required /></label>
						<label class="fieldset"><span class="fieldset-legend">{t('priority-medium')}</span><input class="input w-full" type="number" step="1" bind:value={priorityMedium} required /></label>
						<label class="fieldset"><span class="fieldset-legend">{t('priority-high')}</span><input class="input w-full" type="number" step="1" bind:value={priorityHigh} required /></label>
					</div>
				{/if}
			</fieldset>
			<label class="flex flex-col gap-1">
				<span class="text-xs text-base-content/70">{t('backends-field-models')}</span>
				<input class="input font-mono w-full" bind:value={models} />
			</label>
			<label class="flex flex-col gap-1">
				<span class="text-xs text-base-content/70">{t('backends-field-aliases')}</span>
				<textarea bind:this={aliasesInput} class="textarea font-mono w-full" rows="3" bind:value={aliases}></textarea>
			</label>
			<label class="flex flex-col gap-1">
				<span class="text-xs text-base-content/70">{t('backends-field-extra-headers')}</span>
				<textarea class="textarea font-mono w-full" rows="3" bind:value={extraHeaders}></textarea>
				<span class="text-xs text-base-content/50">{t('backends-field-extra-headers-hint')}</span>
			</label>
			<div class="flex flex-wrap gap-4">
				<label class="label gap-2"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={supportsEdit} /><span>{t('backends-field-supports-edit')}</span></label>
			</div>
		</div>
	</details>
	<div class="sticky bottom-0 z-10 flex items-center gap-2 border-t border-base-300 bg-base-100 py-3">
		{#if backend}
			<button class="btn btn-ghost btn-sm text-error" type="button" disabled={busy} onclick={() => void remove()}>
				{t('backends-delete-backend')}
			</button>
		{/if}
		<span class="flex-1"></span>
		{#if onCancel}<button class="btn btn-ghost btn-sm" type="button" onclick={onCancel}>{t('upstreams-cancel')}</button>{/if}
		<button class="btn btn-primary btn-sm" type="submit" disabled={busy || !name.trim() || !baseUrl.trim() || (nameTaken && !overwrite)}>
			{t('backends-save-backend')}
		</button>
	</div>
</form>

<style>
	.advanced summary {
		cursor: pointer;
		font-size: 0.75rem;
		font-weight: 600;
		letter-spacing: 0.03em;
		text-transform: uppercase;
		color: color-mix(in oklab, var(--color-base-content) 60%, transparent);
	}
	.advanced summary:hover {
		color: var(--color-base-content);
	}
</style>
