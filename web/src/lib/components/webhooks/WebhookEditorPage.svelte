<script lang="ts">
	import { onMount } from 'svelte';
	import { base } from '$app/paths';
	import { goto } from '$app/navigation';
	import { adminJson } from '$lib/admin-client';
	import WebhookForm from './WebhookForm.svelte';
	import WebhookSecretReveal from './WebhookSecretReveal.svelte';
	import { t } from '$lib/i18n.svelte';
	import type { Webhook, WebhooksData } from '$lib/webhooks';

	/**
	 * The webhook editor, on its own route.
	 *
	 * The create form used to sit permanently expanded above the list — a
	 * prompt box, a model picker and four toggles, so the webhooks a user came
	 * to look at started below the fold. It gets a page now, and `/webhooks`
	 * gets an Add button, the same move `/scheduled` made.
	 *
	 * Creating is the one case that cannot just navigate away: the trigger URL
	 * is shown exactly once, and a secret must not travel in the query string
	 * of a redirect, where it would land in history and any access log. So the
	 * form is replaced in place by the reveal, and the user leaves when they
	 * have copied it.
	 */
	let { id = null }: { id?: string | null } = $props();

	let data = $state<WebhooksData | null>(null);
	let webhook = $state<Webhook | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let missing = $state(false);
	let secretUrl = $state<string | null>(null);

	function revealed(secret: string) {
		secretUrl = `${location.origin}/hooks/${secret}`;
	}

	onMount(async () => {
		try {
			data = await adminJson<WebhooksData>('/api/v0/webhooks');
			if (id !== null) {
				webhook = data.webhooks.find((candidate) => candidate.id === id) ?? null;
				missing = webhook === null;
			}
			error = null;
		} catch (caught) {
			error = String(caught);
		} finally {
			loading = false;
		}
	});
</script>

<div class="w-full max-w-4xl">
	<a class="link link-hover text-sm text-base-content/60" href="{base}/webhooks">← {t('webhooks-back')}</a>
	<h1 class="m-0 mt-2 text-2xl font-bold">
		{id === null ? t('webhooks-create-heading') : webhook ? t('webhooks-edit-named-heading', { name: webhook.name }) : t('webhooks-edit-heading')}
	</h1>

	{#if error}<div class="alert alert-error mt-4 text-sm"><span>{error}</span></div>{/if}

	{#if secretUrl}
		<div class="mt-5">
			<WebhookSecretReveal url={secretUrl} />
			<a class="btn btn-primary btn-sm" href="{base}/webhooks?notice=created">{t('webhooks-reveal-done')}</a>
		</div>
	{:else if missing}
		<div class="alert alert-warning mt-4 text-sm"><span>{t('webhooks-toast-not-found')}</span></div>
	{:else if loading || !data}
		<div class="skeleton mt-5 h-96 w-full"></div>
	{:else}
		<div class="mt-5">
			<WebhookForm
				{webhook}
				models={data.models}
				onsaved={async () => {
					// Editing is done; creating stays put until the secret is copied.
					if (id !== null) await goto(`${base}/webhooks?notice=saved`);
				}}
				oncancel={() => goto(`${base}/webhooks`)}
				onsecret={revealed}
			/>
		</div>
	{/if}
</div>
