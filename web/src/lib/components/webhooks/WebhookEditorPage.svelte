<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import WebhookForm from './WebhookForm.svelte';
	import WebhookSecretReveal from './WebhookSecretReveal.svelte';
	import { t } from '#lib/i18n.svelte.js';
	import { webhooksApi, type Webhook, type WebhooksData } from '#lib/webhooks.js';

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
	let disposed = false;
	onDestroy(() => { disposed = true; });
	const listHref = $derived(`/webhooks${data?.principal_id || page.url.searchParams.get('principal_id') ? `?principal_id=${encodeURIComponent(data?.principal_id ?? page.url.searchParams.get('principal_id')!)}` : ''}`);
	const doneHref = $derived(`${listHref}${data?.principal_id ? '&' : '?'}notice=${id === null ? 'created' : 'saved'}`);

	function revealed(secret: string) {
		if (!disposed) secretUrl = `${location.origin}/hooks/${secret}`;
	}

	onMount(async () => {
		try {
			const loaded = await webhooksApi.list(page.url.searchParams.get('principal_id'));
			if (disposed) return;
			data = loaded;
			if (id !== null) {
				webhook = data.webhooks.find((candidate) => candidate.id === id) ?? null;
				missing = webhook === null;
			}
			error = null;
		} catch (caught) {
			if (!disposed) error = String(caught);
		} finally {
			if (!disposed) loading = false;
		}
	});
</script>

<div class="w-full max-w-4xl">
	<a class="link link-hover text-sm text-base-content/60" href={listHref}>← {t('webhooks-back')}</a>
	<h1 class="m-0 mt-2 text-2xl font-bold">
		{id === null ? t('webhooks-create-heading') : webhook ? t('webhooks-edit-named-heading', { name: webhook.name }) : t('webhooks-edit-heading')}
	</h1>

	{#if error}<div class="alert alert-error mt-4 text-sm"><span>{error}</span></div>{/if}

	{#if secretUrl}
		<div class="mt-5">
			<WebhookSecretReveal url={secretUrl} />
			<a class="btn btn-primary btn-sm" href={doneHref}>{t('webhooks-reveal-done')}</a>
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
				agents={data.agents}
				presetAgentId={id === null ? page.url.searchParams.get('personal_agent_id') : null}
				principalId={data.principal_id}
				versions={data.versions}
				canWrite={data.can_write}
				onsaved={async () => {
					// Editing is done; creating stays put until the secret is copied.
					if (!disposed && id !== null) await goto(doneHref);
				}}
				oncancel={() => goto(listHref)}
				onsecret={revealed}
			/>
		</div>
	{/if}
</div>
