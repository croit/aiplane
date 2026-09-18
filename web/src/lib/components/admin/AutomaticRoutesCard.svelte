<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { base } from '$app/paths';
	import { adminDelete, adminJson, adminPut } from '$lib/admin-client';
	import { editableRoute, newRouteBlocker } from '$lib/automatic-routes';
	import { t } from '$lib/i18n.svelte';

	type Candidate = { key: string; target: string; description: string };
	type Route = {
		alias: string;
		selector_model: string;
		objective: 'quality' | 'balanced' | 'cost';
		instructions: string;
		minimum_confidence: number;
		selector_timeout_ms: number;
		fallback_target: string;
		session_affinity: boolean;
		session_ttl_seconds: number;
		rollout: 'active' | 'shadow';
		version: number;
		candidates: Candidate[];
	};
	type Decision = {
		created_at: string;
		route_alias: string;
		route_version: number;
		selected_target: string | null;
		effective_target: string;
		confidence: number | null;
		reason: string;
		duration_ms: number;
	};
	type Data = { routes: Route[]; candidate_models: string[]; selector_models: string[]; decisions: Decision[] };

	let data = $state<Data | null>(null);
	let editing = $state<Route | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let saving = $state(false);
	let blocker = $derived(data ? newRouteBlocker(data.selector_models, data.candidate_models) : null);
	let card: HTMLElement;
	let editor = $state<HTMLFormElement | undefined>();

	function emptyRoute(source: Data): Route {
		return {
			alias: '',
			selector_model: source.selector_models[0] ?? '',
			objective: 'balanced',
			instructions: '',
			minimum_confidence: 0.7,
			selector_timeout_ms: 1500,
			fallback_target: source.candidate_models[0] ?? '',
			session_affinity: false,
			session_ttl_seconds: 3600,
			rollout: 'shadow',
			version: 0,
			candidates: source.candidate_models.slice(0, 2).map((target, index) => ({
				key: index === 0 ? 'fast' : 'expert', target, description: ''
			}))
		};
	}

	async function refresh() {
		try {
			data = await adminJson<Data>('/api/v0/admin/automatic-routes');
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	async function showEditor(route: Route) {
		editing = editableRoute(route);
		notice = null;
		await tick();
		editor?.scrollIntoView({ block: 'start' });
	}

	async function addRoute(source: Data) {
		editing = emptyRoute(source);
		notice = null;
		await tick();
		editor?.scrollIntoView({ block: 'start' });
	}

	async function cancel() {
		editing = null;
		await tick();
		card.scrollIntoView({ block: 'start' });
	}

	function objectiveLabel(objective: Route['objective']) {
		return t(`auto-route-objective-${objective}`);
	}

	function rolloutLabel(rollout: Route['rollout']) {
		return t(`auto-route-rollout-${rollout}`);
	}

	function reasonLabel(reason: string) {
		switch (reason) {
			case 'selected': return t('auto-route-reason-selected');
			case 'shadow': return t('auto-route-reason-shadow');
			case 'low_confidence': return t('auto-route-reason-low-confidence');
			case 'selector_error': return t('auto-route-reason-selector-error');
			case 'session_affinity': return t('auto-route-reason-session-affinity');
			default: return reason;
		}
	}

	function addCandidate() {
		if (!editing) return;
		editing.candidates = [...editing.candidates, { key: '', target: '', description: '' }];
	}

	function removeCandidate(index: number) {
		const route = editing;
		if (!route) return;
		route.candidates = route.candidates.filter((_, candidateIndex) => candidateIndex !== index);
		if (!route.candidates.some((candidate) => candidate.target === route.fallback_target)) {
			route.fallback_target = route.candidates[0]?.target ?? '';
		}
	}

	async function save() {
		if (!editing) return;
		saving = true;
		try {
			await adminPut('/api/v0/admin/automatic-routes', editing);
			notice = t('auto-route-saved', { alias: editing.alias });
			editing = null;
			await refresh();
			await tick();
			card.scrollIntoView({ block: 'start' });
		} catch (caught) {
			error = String(caught);
		} finally {
			saving = false;
		}
	}

	async function remove(alias: string) {
		if (!confirm(t('auto-route-delete-confirm', { alias }))) return;
		try {
			await adminDelete(`/api/v0/admin/automatic-routes/${encodeURIComponent(alias)}`);
			notice = t('auto-route-deleted', { alias });
			if (editing?.alias === alias) editing = null;
			await refresh();
		} catch (caught) {
			error = String(caught);
		}
	}

	onMount(refresh);
</script>

<article class="card card-border bg-base-100" bind:this={card}>
	<div class="card-body gap-4">
		<header class="flex flex-col gap-1 sm:flex-row sm:items-start sm:justify-between">
			<div>
				<h2 class="card-title text-base">{t('auto-route-heading')}</h2>
				<p class="text-sm text-base-content/70">{t('auto-route-description')}</p>
			</div>
			{#if data && !editing}<button class="btn btn-primary btn-sm" type="button" disabled={blocker !== null} onclick={() => void addRoute(data!)}>+ {t('auto-route-add')}</button>{/if}
		</header>

		<div class="alert alert-warning alert-soft" role="note"><span>{t('auto-route-privacy')}</span></div>
		{#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}
		{#if notice}<div class="alert alert-success" role="status"><span>{notice}</span></div>{/if}

		{#if data}
			{#if blocker}
				<div class="alert alert-info alert-soft" role="status">
					<span>{t(blocker === 'selector' ? 'auto-route-needs-selector' : 'auto-route-needs-candidates')}</span>
					<a class="btn btn-sm" href="{base}/admin/upstreams">{t('auto-route-open-upstreams')}</a>
				</div>
			{/if}
			{#if data.routes.length === 0 && !editing}<p class="text-sm text-base-content/60">{t('auto-route-empty')}</p>{/if}
			<div class="grid gap-3 lg:grid-cols-2">
				{#each data.routes as route (route.alias)}
					<section class="card card-border card-sm">
						<div class="card-body gap-2">
							<div class="flex items-center justify-between gap-2">
								<h3 class="font-mono font-semibold">{route.alias}</h3>
								<div class="flex gap-1"><span class="badge badge-outline">v{route.version}</span><span class="badge">{rolloutLabel(route.rollout)}</span></div>
							</div>
							<p class="text-sm text-base-content/70">{route.candidates.length} {t('auto-route-candidates')} · {objectiveLabel(route.objective)} · {route.fallback_target}</p>
							<div class="card-actions justify-end">
								<button class="btn btn-sm" type="button" onclick={() => void showEditor(route)}>{t('auto-route-edit')}</button>
								<button class="btn btn-ghost btn-sm text-error" type="button" onclick={() => void remove(route.alias)}>{t('auto-route-delete')}</button>
							</div>
						</div>
					</section>
				{/each}
			</div>

			{#if editing}
				<form class="card card-border bg-base-200" bind:this={editor} onsubmit={(event) => { event.preventDefault(); void save(); }}>
					<div class="card-body gap-4">
						<div>
							<h3 class="card-title text-base">{editing.version ? t('auto-route-edit') : t('auto-route-add')}</h3>
							<p class="text-sm text-base-content/70">{t('auto-route-editor-help')}</p>
						</div>
						<div class="grid gap-3 md:grid-cols-2">
							<fieldset class="fieldset w-full min-w-0"><legend class="fieldset-legend">{t('auto-route-alias')}</legend><input class="input w-full" required bind:value={editing.alias} disabled={editing.version > 0} /><p class="label whitespace-normal">{t('auto-route-alias-help')}</p></fieldset>
							<fieldset class="fieldset w-full min-w-0"><legend class="fieldset-legend">{t('auto-route-selector')}</legend><select class="select w-full" required bind:value={editing.selector_model}>{#if !data.selector_models.includes(editing.selector_model)}<option value={editing.selector_model}>{editing.selector_model}</option>{/if}{#each data.selector_models as model}<option value={model}>{model}</option>{/each}</select><p class="label whitespace-normal">{t('auto-route-selector-help')}</p></fieldset>
							<fieldset class="fieldset w-full min-w-0"><legend class="fieldset-legend">{t('auto-route-objective')}</legend><select class="select w-full" bind:value={editing.objective}><option value="quality">{t('auto-route-objective-quality')}</option><option value="balanced">{t('auto-route-objective-balanced')}</option><option value="cost">{t('auto-route-objective-cost')}</option></select></fieldset>
							<fieldset class="fieldset w-full min-w-0"><legend class="fieldset-legend">{t('auto-route-rollout')}</legend><select class="select w-full" bind:value={editing.rollout}><option value="shadow">{t('auto-route-rollout-shadow')}</option><option value="active">{t('auto-route-rollout-active')}</option></select><p class="label whitespace-normal">{t('auto-route-rollout-help')}</p></fieldset>
							<fieldset class="fieldset w-full min-w-0"><legend class="fieldset-legend">{t('auto-route-confidence')}</legend><input class="input w-full" type="number" min="0" max="1" step="0.01" bind:value={editing.minimum_confidence} /><p class="label whitespace-normal">{t('auto-route-confidence-help')}</p></fieldset>
							<fieldset class="fieldset w-full min-w-0"><legend class="fieldset-legend">{t('auto-route-timeout')}</legend><input class="input w-full" type="number" min="100" max="30000" step="100" bind:value={editing.selector_timeout_ms} /></fieldset>
						</div>
						<fieldset class="fieldset w-full min-w-0"><legend class="fieldset-legend">{t('auto-route-instructions')}</legend><textarea class="textarea min-h-24 w-full" bind:value={editing.instructions}></textarea></fieldset>
						<div class="flex flex-col gap-3">
							<div class="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between"><div><h4 class="font-semibold">{t('auto-route-candidates')}</h4><p class="text-sm text-base-content/70">{t('auto-route-candidates-help')}</p></div><button class="btn btn-sm" type="button" onclick={addCandidate}>+ {t('auto-route-candidate-add')}</button></div>
							{#each editing.candidates as candidate, index}
								<div class="grid items-start gap-2 rounded-box border border-base-300 p-3 md:grid-cols-[1fr_1.4fr_2fr_auto]">
									<fieldset class="fieldset w-full min-w-0"><legend class="fieldset-legend">{t('auto-route-candidate-key')}</legend><input class="input w-full" required bind:value={candidate.key} /></fieldset>
									<fieldset class="fieldset w-full min-w-0"><legend class="fieldset-legend">{t('auto-route-candidate-target')}</legend><select class="select w-full" required bind:value={candidate.target}>{#if !data.candidate_models.includes(candidate.target)}<option value={candidate.target}>{candidate.target}</option>{/if}{#each data.candidate_models as model}<option value={model}>{model}</option>{/each}</select></fieldset>
									<fieldset class="fieldset w-full min-w-0"><legend class="fieldset-legend">{t('auto-route-candidate-description')}</legend><input class="input w-full" required bind:value={candidate.description} /></fieldset>
									<button class="btn btn-ghost btn-sm text-error md:mt-7" type="button" onclick={() => removeCandidate(index)}>{t('auto-route-delete')}</button>
								</div>
							{/each}
						</div>
						<div class="grid gap-3 md:grid-cols-2">
							<fieldset class="fieldset w-full min-w-0"><legend class="fieldset-legend">{t('auto-route-fallback')}</legend><select class="select w-full" required bind:value={editing.fallback_target}>{#each editing.candidates as candidate}<option value={candidate.target}>{candidate.target || candidate.key}</option>{/each}</select></fieldset>
							<fieldset class="fieldset w-full min-w-0"><legend class="fieldset-legend">{t('auto-route-session-ttl')}</legend><input class="input w-full" type="number" min="60" max="604800" bind:value={editing.session_ttl_seconds} disabled={!editing.session_affinity} /></fieldset>
						</div>
						<label class="label w-full min-w-0 cursor-pointer items-start justify-start gap-3 whitespace-normal"><input class="toggle toggle-primary mt-0.5 shrink-0" type="checkbox" bind:checked={editing.session_affinity} /><span class="min-w-0"><span class="block font-medium">{t('auto-route-session-affinity')}</span><span class="block text-sm text-base-content/70">{t('auto-route-session-affinity-help')}</span></span></label>
						<div class="card-actions justify-end"><button class="btn btn-ghost" type="button" onclick={() => void cancel()}>{t('auto-route-cancel')}</button><button class="btn btn-primary" type="submit" disabled={saving || editing.candidates.length < 2}>{saving ? t('auto-route-saving') : t('auto-route-save')}</button></div>
					</div>
				</form>
			{/if}

			{#if data.decisions.length}
				<details>
					<summary class="cursor-pointer font-semibold">{t('auto-route-decisions')}</summary>
						<div class="mt-2 overflow-x-auto"><table class="table table-sm"><thead><tr><th>{t('auto-route-alias')}</th><th>{t('auto-route-result')}</th><th>{t('auto-route-confidence')}</th><th>{t('auto-route-latency')}</th></tr></thead><tbody>{#each data.decisions as decision}<tr><td class="font-mono">{decision.route_alias} v{decision.route_version}</td><td>{decision.effective_target} <span class="badge badge-outline badge-sm">{reasonLabel(decision.reason)}</span></td><td>{decision.confidence === null ? '—' : decision.confidence.toFixed(2)}</td><td>{decision.duration_ms} ms</td></tr>{/each}</tbody></table></div>
				</details>
			{/if}
		{/if}
	</div>
</article>
