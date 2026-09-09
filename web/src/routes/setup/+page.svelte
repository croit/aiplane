<script lang="ts">
	import { onMount } from 'svelte';
	import { base } from '$app/paths';
	import { t } from '$lib/i18n.svelte';

	interface Draft {
		public_url: string;
		issuer: string;
		client_id: string;
		client_secret_set: boolean;
		scopes: string[];
		roles_claim: string | null;
	}
	interface Proof {
		subject: string;
		email: string;
		name: string | null;
		claims: Record<string, unknown>;
	}
	interface State {
		access: 'first_run' | 'recovery' | 'closed';
		draft: Draft | null;
		proof: Proof | null;
		suggested_public_url: string;
	}

	let wiz = $state<State | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let busy = $state(false);

	let fpublicUrl = $state('');
	let fissuer = $state('');
	let fclientId = $state('');
	let fsecret = $state('');
	let fscopes = $state('openid email profile');
	let frolesClaim = $state('groups');

	// The admin-claim picker: derived from the proof's claims.
	let picked = $state('');
	let manualClaim = $state('');
	let manualValue = $state('');

	async function refresh() {
		try {
			// A reopened wizard (`restore-setup` on the host) is token-gated: the
			// operator arrives at `/setup?claim=<one-time token>`. Forward it on
			// the first read — the response sets the `gw_setup` cookie that
			// carries the claim through the rest of the run, so it only has to
			// ride on this one request. A first run has no token and needs none.
			const claim = new URLSearchParams(location.search).get('claim');
			const url = claim
				? `/api/v0/setup/state?claim=${encodeURIComponent(claim)}`
				: '/api/v0/setup/state';
			const res = await fetch(url, { headers: { accept: 'application/json' } });
			const body = await res.json();
			if (!res.ok) throw new Error(body?.error?.message ?? res.statusText);
			wiz = body;
			if (body.draft && !fissuer) {
				fpublicUrl = body.draft.public_url;
				fissuer = body.draft.issuer;
				fclientId = body.draft.client_id;
				fscopes = (body.draft.scopes ?? []).join(' ');
				frolesClaim = body.draft.roles_claim ?? 'groups';
			} else if (!fpublicUrl) {
				fpublicUrl = body.suggested_public_url;
			}
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function testLogin() {
		busy = true;
		notice = null;
		error = null;
		try {
			const res = await fetch('/api/v0/setup/test', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({
					public_url: fpublicUrl,
					issuer: fissuer,
					client_id: fclientId,
					client_secret: fsecret,
					scopes: fscopes.split(/\s+/).filter(Boolean),
					roles_claim: frolesClaim
				})
			});
			const body = await res.json();
			if (!res.ok) throw new Error(body?.error?.message ?? res.statusText);
			// Navigate the browser into the provider's authorization URL; the
			// callback lands back on /app/setup with the proof recorded.
			location.href = body.authorize_url;
		} catch (err) {
			error = String(err);
		} finally {
			busy = false;
		}
	}

	async function restart() {
		await fetch('/api/v0/setup/restart', { method: 'POST' });
		await refresh();
	}

	async function finish() {
		busy = true;
		error = null;
		try {
			const res = await fetch('/api/v0/setup/finish', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({
					claim: picked.split('\u0000')[0] || null,
					value: picked.split('\u0000')[1] || null,
					manual_claim: manualClaim,
					manual_value: manualValue
				})
			});
			const body = await res.json();
			if (!res.ok) throw new Error(body?.error?.message ?? res.statusText);
			location.href = `${base}/admin/settings`;
		} catch (err) {
			error = String(err);
		} finally {
			busy = false;
		}
	}

	/** Every string / string[] claim in the proof, flattened as pickable pairs. */
	const claimChoices = $derived.by(() => {
		const out: { claim: string; value: string; label: string }[] = [];
		const claims = wiz?.proof?.claims ?? {};
		for (const [key, raw] of Object.entries(claims)) {
			if (Array.isArray(raw)) {
				for (const v of raw) {
					if (typeof v === 'string' && v) out.push({ claim: key, value: v, label: `${key} = ${v}` });
				}
			} else if (typeof raw === 'string' && raw && !key.startsWith('_')) {
				out.push({ claim: key, value: raw, label: `${key} = ${raw}` });
			}
		}
		return out;
	});

	// What the provider must be told to allow. Built from the URL field as the
	// operator types, because it is only correct if it matches that field
	// exactly — which is the whole reason the field has its own warning.
	const redirectUri = $derived(
		fpublicUrl.trim() ? `${fpublicUrl.trim().replace(/\/+$/, '')}/auth/callback` : ''
	);

	onMount(refresh);
</script>

<div class="mx-auto max-w-xl">
	<h1 class="text-2xl font-bold mb-2">{t('setup-page-heading')}</h1>

	{#if error}<div class="alert alert-error mb-4 text-sm"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-warning mb-4 text-sm"><span>{notice}</span></div>{/if}

	{#if wiz?.access === 'closed'}
		<div class="alert alert-info mb-4"><span>{t('setup-closed')}</span></div>
	{:else if wiz?.proof}
		<!-- Screen 2: the test login proved the provider; pick the admin claim. -->
		<div class="card border border-base-300 mb-4">
			<div class="card-body">
				<span class="text-xs uppercase tracking-wide text-base-content/50">{t('setup-step-2-of-2')}</span>
				<h2 class="card-title text-base">{t('setup-admin-heading')}</h2>
				<p class="text-sm text-base-content/70">
					{t('setup-login-worked')}
					<strong>{wiz?.proof?.email || wiz?.proof?.subject}</strong>
				</p>
				<p class="text-sm text-base-content/70">{t('setup-admin-intro')}</p>

				{#if claimChoices.length === 0}
					<div class="alert alert-warning text-sm mt-2"><span>{t('setup-no-claims')}</span></div>
				{:else}
					<div class="flex flex-col gap-1 mt-2">
						{#each claimChoices as choice (choice.label)}
							<label class="label cursor-pointer justify-start gap-2">
								<input
									type="radio"
									class="radio radio-sm"
									name="admin-claim"
									value="{choice.claim}&#0;{choice.value}"
									bind:group={picked}
								/>
								<span class="label-text font-mono text-xs">{choice.label}</span>
							</label>
						{/each}
					</div>
				{/if}

				<div class="divider text-xs">{t('setup-or-manual')}</div>
				<div class="flex gap-2">
					<label class="flex flex-col gap-1 flex-1">
						<span class="label-text text-xs">{t('setup-manual-claim')}</span>
						<input class="input input-bordered input-sm" placeholder="groups" bind:value={manualClaim} />
					</label>
					<label class="flex flex-col gap-1 flex-1">
						<span class="label-text text-xs">{t('setup-manual-value')}</span>
						<input class="input input-bordered input-sm" placeholder="gateway-admins" bind:value={manualValue} />
					</label>
				</div>
				<p class="text-xs text-base-content/60">{t('setup-manual-help')}</p>

				<details class="mt-2">
					<summary class="text-xs cursor-pointer text-base-content/60">{t('setup-show-token')}</summary>
					<pre class="mt-1 max-h-64 overflow-auto rounded bg-base-200 p-2 text-xs">{JSON.stringify(
							wiz?.proof?.claims ?? {},
							null,
							2
						)}</pre>
				</details>

				<div class="card-actions justify-end mt-2">
					<button class="btn btn-ghost btn-sm" onclick={restart}>{t('setup-back-button')}</button>
					<button
						class="btn btn-primary btn-sm"
						onclick={finish}
						disabled={busy || (!picked && !(manualClaim.trim() && manualValue.trim()))}
					>
						{t('setup-finish-button')}
					</button>
				</div>
			</div>
		</div>
	{:else}
		<!-- Screen 1: provider settings. -->
		<div class="card border border-base-300">
			<div class="card-body">
				<span class="text-xs uppercase tracking-wide text-base-content/50">{t('setup-step-1-of-2')}</span>
				<h2 class="card-title text-base">{t('setup-provider-heading')}</h2>
				<p class="text-sm text-base-content/70">{t('setup-provider-intro')}</p>
				<div class="flex flex-col gap-3 mt-2">
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('setup-field-public-url')}</span>
						<input class="input input-bordered input-sm" bind:value={fpublicUrl} placeholder="https://gw.example.com" />
						<span class="text-xs text-base-content/60">{t('setup-field-public-url-help')}</span>
					</label>

					{#if redirectUri}
						<div class="rounded border border-warning/40 bg-warning/10 p-2">
							<p class="text-xs font-semibold">{t('setup-redirect-uri-heading')}</p>
							<code class="mt-1 block break-all text-xs">{redirectUri}</code>
							<p class="mt-1 text-xs text-base-content/70">{t('setup-redirect-uri-help')}</p>
						</div>
					{/if}

					<label class="flex flex-col gap-1">
						<span class="label-text">{t('setup-field-issuer')}</span>
						<input class="input input-bordered input-sm" bind:value={fissuer} placeholder="https://id.example.com" />
						<span class="text-xs text-base-content/60">{t('setup-field-issuer-help')}</span>
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('setup-field-client-id')}</span>
						<input class="input input-bordered input-sm" bind:value={fclientId} />
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">
							{t('setup-field-client-secret')}{wiz?.draft?.client_secret_set
								? ` (${t('setup-secret-set-hint')})`
								: ''}
						</span>
						<input class="input input-bordered input-sm" type="password" bind:value={fsecret} />
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('setup-field-scopes')}</span>
						<input class="input input-bordered input-sm" bind:value={fscopes} />
						<span class="text-xs text-base-content/60">{t('setup-field-scopes-help')}</span>
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">{t('setup-field-roles-claim')}</span>
						<input class="input input-bordered input-sm" bind:value={frolesClaim} placeholder="groups" />
						<span class="text-xs text-base-content/60">{t('setup-field-roles-claim-help')}</span>
					</label>
				</div>
				<div class="card-actions items-center justify-end mt-3 gap-2">
					<span class="text-xs text-base-content/60">{t('setup-test-button-help')}</span>
					<button
						class="btn btn-primary btn-sm"
						onclick={testLogin}
						disabled={busy || !fpublicUrl.trim() || !fissuer.trim() || !fclientId.trim()}
					>
						{busy ? t('setup-testing') : t('setup-test-button')}
					</button>
				</div>
			</div>
		</div>
	{/if}
</div>
