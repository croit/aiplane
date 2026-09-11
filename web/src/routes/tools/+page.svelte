<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost } from '$lib/admin-client';
	import LocationSharingCard from '$lib/components/tools/LocationSharingCard.svelte';
	import ToolToggleSections from '$lib/components/tools/ToolToggleSections.svelte';
	import { t } from '$lib/i18n.svelte';
	import { currentBrowserLocation, type LocationSharingState, type ToolEntry, type ToolsResponse } from '$lib/tools';

	let tools = $state<ToolEntry[]>([]);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let saving = $state<string | null>(null);
	let location = $state<LocationSharingState | null>(null);
	let locationBusy = $state(false);

	const sections = $derived.by(() => {
		const byCategory = new Map<string, ToolEntry[]>();
		for (const tool of tools) {
			const list = byCategory.get(tool.category) ?? [];
			list.push(tool);
			byCategory.set(tool.category, list);
		}
		return [...byCategory.entries()];
	});

	async function refresh() {
		try {
			const data = await adminJson<ToolsResponse>('/api/v0/tools');
			tools = data.tools;
			location = data.location;
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function shareLocation() {
		locationBusy = true;
		notice = null;
		try {
			const position = await currentBrowserLocation();
			await adminJson('/api/v0/me/location', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify(position)
			});
			location = { shared: true, accuracy: position.accuracy };
		} catch {
			notice = t('tools-location-unavailable');
		} finally {
			locationBusy = false;
		}
	}

	async function forgetLocation() {
		locationBusy = true;
		notice = null;
		try {
			await adminJson('/api/v0/me/location', { method: 'DELETE' });
			location = { shared: false, accuracy: null };
		} catch (err) {
			notice = String(err);
		} finally {
			locationBusy = false;
		}
	}

	async function toggle(tool: ToolEntry) {
		if (saving) return;
		saving = tool.key;
		notice = null;
		try {
			await adminPost('/api/v0/tools/toggle', {
				tool_key: tool.key,
				enabled: !tool.enabled
			});
			tool.enabled = !tool.enabled;
		} catch (err) {
			notice = String(err);
		} finally {
			saving = null;
		}
	}

	onMount(refresh);
</script>

<div class="mx-auto w-full max-w-5xl px-4 pb-6 pt-14 sm:px-6 sm:pt-6">
	<h1 class="mb-2 text-2xl font-bold">{t('tools-heading')}</h1>
	<p class="mb-6 text-sm text-base-content/60">{t('tools-description')}</p>

{#if error}
	<div class="alert alert-error mb-4"><span>{error}</span></div>
{/if}
{#if notice}
	<div class="alert alert-warning mb-4"><span>{notice}</span></div>
{/if}

	{#if location}<LocationSharingCard {location} busy={locationBusy} onshare={shareLocation} onforget={forgetLocation} />{/if}
	{#if sections.length > 0}
		<ToolToggleSections {sections} {saving} ontoggle={toggle} />
	{:else if !error}
		<div class="card border border-base-300"><div class="card-body"><p class="m-0 text-sm text-base-content/60">{t('tools-none-granted')}</p></div></div>
	{/if}
</div>
