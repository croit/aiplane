<script lang="ts">
	import { untrack } from 'svelte';
	import { t } from '$lib/i18n.svelte';

	let { search, onsave }: {
		search: { provider: string; searxng_url: string | null; brave_key_set: boolean };
		onsave: (settings: { provider: string; searxng_url: string; brave_api_key: string; clear_brave_key: boolean }) => Promise<void>;
	} = $props();

	let provider = $state(untrack(() => search.provider));
	let searxngUrl = $state(untrack(() => search.searxng_url ?? ''));
	let braveKey = $state('');
	let clearBraveKey = $state(false);
	let saving = $state(false);
	let error = $state<string | null>(null);

	async function save() {
		saving = true;
		error = null;
		try {
			await onsave({ provider, searxng_url: searxngUrl, brave_api_key: braveKey, clear_brave_key: clearBraveKey });
			braveKey = '';
			clearBraveKey = false;
		} catch (caught) {
			error = String(caught);
		} finally {
			saving = false;
		}
	}
</script>

<article class="card border border-base-300 bg-base-100">
	<div class="card-body gap-3">
		<header class="flex flex-col gap-1">
			<h2 class="card-title text-base">{t('admin-search-heading')}</h2>
			<p class="text-sm text-base-content/70">{t('admin-search-intro')}</p>
		</header>
		{#if error}<div class="alert alert-error py-2 text-sm"><span>{error}</span></div>{/if}
		<form class="m-0 flex flex-col gap-3" onsubmit={(event) => { event.preventDefault(); save(); }}>
			<div class="grid grid-cols-1 gap-4 md:grid-cols-2">
				<label class="flex flex-col gap-1">
					<span class="label-text text-xs">{t('admin-search-provider-label')}</span>
					<select class="select select-bordered select-sm w-full" aria-label={t('admin-search-provider-label')} bind:value={provider}>
						<option value="searxng">{t('admin-search-provider-searxng')}</option>
						<option value="brave">{t('admin-search-provider-brave')}</option>
					</select>
				</label>
				<label class="flex flex-col gap-1">
					<span class="label-text text-xs">{t('admin-search-searxng-url-label')}</span>
					<input type="url" class="input input-bordered input-sm w-full" aria-label={t('admin-search-searxng-url-label')} bind:value={searxngUrl} placeholder={t('admin-search-searxng-url-placeholder')} />
				</label>
			</div>
			<div class="flex flex-col gap-1">
				<label class="flex flex-col gap-1">
					<span class="label-text text-xs">{t('admin-search-brave-key-label')}</span>
					<input type="password" autocomplete="off" class="input input-bordered input-sm w-full" aria-label={t('admin-search-brave-key-label')} bind:value={braveKey} placeholder={t('admin-search-brave-key-placeholder')} />
				</label>
				<span class="text-xs text-base-content/60">{t(search.brave_key_set ? 'admin-search-brave-key-set' : 'admin-search-brave-key-unset')}</span>
				{#if search.brave_key_set}
					<label class="label cursor-pointer justify-start gap-2"><input type="checkbox" class="checkbox checkbox-sm" bind:checked={clearBraveKey} /><span>{t('admin-search-brave-key-clear')}</span></label>
				{/if}
			</div>
			<div class="flex justify-end"><button class="btn btn-primary btn-sm" type="submit" disabled={saving}>{t('admin-search-save')}</button></div>
		</form>
	</div>
</article>
