<script lang="ts">
	import { ApiError, request } from '#lib/api.js';
	import { t } from '#lib/i18n.svelte.js';

	let { light, dark, onrefresh }: {
		light: boolean;
		dark: boolean;
		onrefresh: () => Promise<void>;
	} = $props();

	type Slot = 'light' | 'dark';

	let saving = $state<Slot | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	// Bumped after every change so the previews re-fetch the new bytes —
	// /brand/* answers no-cache, but the <img> tags would keep their URL.
	let version = $state(0);

	function source(slot: Slot) {
		return `${slot === 'light' ? '/brand/logo' : '/brand/logo-dark'}?v=${version}`;
	}

	function setFile(slot: Slot) {
		return (event: Event) => {
			const input = event.currentTarget as HTMLInputElement;
			const file = input.files?.[0] ?? null;
			input.value = '';
			if (file) void upload(slot, file);
		};
	}

	async function upload(slot: Slot, file: File) {
		saving = slot;
		error = null;
		notice = null;
		const form = new FormData();
		form.append('file', file);
		form.append('slot', slot);
		try {
			await request('/api/v0/admin/branding', { method: 'POST', body: form });
			notice = t('branding-saved');
			version++;
			await onrefresh();
		} catch (caught) {
			error = caught instanceof ApiError ? (caught.serverMessage ?? caught.message) : String(caught);
		} finally {
			saving = null;
		}
	}

	async function remove(slot: Slot) {
		saving = slot;
		error = null;
		notice = null;
		try {
			await request(`/api/v0/admin/branding/${slot}`, { method: 'DELETE' });
			notice = t('branding-removed');
			version++;
			await onrefresh();
		} catch (caught) {
			error = caught instanceof ApiError ? (caught.serverMessage ?? caught.message) : String(caught);
		} finally {
			saving = null;
		}
	}

	const slots: Slot[] = ['light', 'dark'];
</script>

<article class="card">
	<div class="card-body gap-3">
		<header class="flex flex-col gap-1">
			<h2 class="card-title text-base">{t('branding-heading')}</h2>
			<p class="text-sm text-base-content/70">{t('branding-intro')}</p>
		</header>
		{#if error}<div class="alert alert-error py-2 text-sm"><span>{error}</span></div>{/if}
		{#if notice}<div class="alert alert-success py-2 text-sm"><span>{notice}</span></div>{/if}
		<div class="flex flex-col gap-4">
			{#each slots as slot (slot)}
				{@const set = slot === 'light' ? light : dark}
				<div class="flex flex-col gap-2 rounded-box border border-base-300 p-3 sm:flex-row sm:items-center">
					<div class="flex h-14 w-40 shrink-0 items-center justify-center overflow-hidden rounded bg-base-200">
						{#if set}
							<img src={source(slot)} alt="" class="max-h-10 max-w-full object-contain" />
						{:else}
							<span class="text-xs text-base-content/40">{t(`branding-slot-${slot}`)}</span>
						{/if}
					</div>
					<div class="flex min-w-0 grow flex-col gap-2">
						<span class="text-sm font-medium">{t(`branding-slot-${slot}`)}</span>
						<div class="flex flex-wrap items-center gap-2">
							<label class="file-input file-input-sm">
								<span class="sr-only">{t('branding-upload')}</span>
								<input type="file" accept="image/svg+xml,image/png,image/jpeg" disabled={saving !== null} onchange={setFile(slot)} />
							</label>
							{#if set}
								<button class="btn btn-ghost btn-sm" disabled={saving !== null} onclick={() => remove(slot)}>{t('branding-remove')}</button>
							{/if}
							{#if saving === slot}<span class="loading loading-spinner loading-xs"></span>{/if}
						</div>
					</div>
				</div>
			{/each}
		</div>
		<p class="m-0 text-xs text-base-content/60">{t('branding-format-hint')}</p>
	</div>
</article>
