<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { adminJson } from '$lib/admin-client';
	import WebhookForm from '$lib/components/webhooks/WebhookForm.svelte';
	import WebhookSubpageHeader from '$lib/components/webhooks/WebhookSubpageHeader.svelte';
	import { t } from '$lib/i18n.svelte';
	import type { Webhook, WebhooksData } from '$lib/webhooks';
	let data = $state<WebhooksData | null>(null);
	let webhook = $state<Webhook | null>(null);
	let error = $state<string | null>(null);
	onMount(async () => {
		try { data = await adminJson<WebhooksData>('/api/v0/webhooks'); webhook = data.webhooks.find((candidate) => candidate.id === page.params.id) ?? null; if (!webhook) error = t('webhooks-toast-not-found'); }
		catch (caught) { error = String(caught); }
	});
</script>

<div class="mx-auto w-full max-w-5xl"><WebhookSubpageHeader title={t('webhooks-edit-heading')} />{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}{#if data && webhook}<WebhookForm {webhook} models={data.models} onsaved={() => goto('/webhooks')} oncancel={() => goto('/webhooks')} />{/if}</div>
