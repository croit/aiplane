<script lang="ts">
	import { onMount } from 'svelte';
	import { base } from '$app/paths';

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
			const res = await fetch('/api/v0/setup/state', { headers: { accept: 'application/json' } });
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

	onMount(refresh);
</script>

<div class="mx-auto max-w-xl">
	<h1 class="text-2xl font-bold mb-2">Gateway setup</h1>

	{#if error}<div class="alert alert-error mb-4 text-sm"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-warning mb-4 text-sm"><span>{notice}</span></div>{/if}

	{#if wiz?.access === 'closed'}
		<div class="alert alert-info mb-4">
			<span>Setup is closed on this gateway. Sign in normally, or run `restore-setup` on the host to reopen the wizard.</span>
		</div>
	{:else if wiz?.proof}
		<!-- Screen 2: the test login proved the provider; pick the admin claim. -->
		<div class="card border border-base-300 mb-4">
			<div class="card-body">
				<h2 class="card-title text-base">Test login succeeded</h2>
				<p class="text-sm text-base-content/70">
					Signed in as <strong>{wiz?.proof?.email || wiz?.proof?.subject}</strong>.
					Pick which claim value should grant administrator access.
				</p>
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
				<div class="divider text-xs">or type a pair</div>
				<div class="flex gap-2">
					<input class="input input-bordered input-sm flex-1" placeholder="claim (e.g. groups)" bind:value={manualClaim} />
					<input class="input input-bordered input-sm flex-1" placeholder="value (e.g. gateway-admins)" bind:value={manualValue} />
				</div>
				<div class="card-actions justify-end mt-2">
					<button class="btn btn-ghost btn-sm" onclick={restart}>Back</button>
					<button
						class="btn btn-primary btn-sm"
						onclick={finish}
						disabled={busy || (!picked && !(manualClaim.trim() && manualValue.trim()))}
					>
						Finish setup
					</button>
				</div>
			</div>
		</div>
	{:else}
		<!-- Screen 1: provider settings. -->
		<div class="card border border-base-300">
			<div class="card-body">
				<h2 class="card-title text-base">Sign-in provider</h2>
				<p class="text-sm text-base-content/70">
					Point the gateway at your OIDC provider and run a real test login.
				</p>
				<div class="flex flex-col gap-3 mt-2">
					<label class="flex flex-col gap-1">
						<span class="label-text">Public URL</span>
						<input class="input input-bordered input-sm" bind:value={fpublicUrl} placeholder="https://gw.example.com" />
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">Issuer</span>
						<input class="input input-bordered input-sm" bind:value={fissuer} placeholder="https://id.example.com" />
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">Client ID</span>
						<input class="input input-bordered input-sm" bind:value={fclientId} />
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">
							Client secret{wiz?.draft?.client_secret_set ? ' (set — blank keeps)' : ''}
						</span>
						<input class="input input-bordered input-sm" type="password" bind:value={fsecret} />
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">Scopes</span>
						<input class="input input-bordered input-sm" bind:value={fscopes} />
					</label>
					<label class="flex flex-col gap-1">
						<span class="label-text">Roles claim (optional)</span>
						<input class="input input-bordered input-sm" bind:value={frolesClaim} placeholder="groups" />
					</label>
				</div>
				<div class="card-actions justify-end mt-3">
					<button
						class="btn btn-primary btn-sm"
						onclick={testLogin}
						disabled={busy || !fpublicUrl.trim() || !fissuer.trim() || !fclientId.trim()}
					>
						{busy ? 'Testing…' : 'Test login'}
					</button>
				</div>
			</div>
		</div>
	{/if}
</div>
