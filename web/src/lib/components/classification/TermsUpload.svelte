<script lang="ts">
	import { classificationApi, csvColumns, failure, type DataClass, type Terms, type TermsPreview } from '#lib/classification.js';
	import { dt, t } from '#lib/i18n.svelte.js';

	/**
	 * A class's word list: choose a file, see what would be taken, then upload.
	 * A CSV asks which column holds the words. The list replaces the previous
	 * one; the words are never shown again, only their count.
	 */
	let { cls, onterms }: { cls: DataClass; onterms: (terms: Terms) => void } = $props();

	let file = $state<File | null>(null);
	let columns = $state<string[]>([]);
	let column = $state('');
	let preview = $state<TermsPreview | null>(null);
	let error = $state<string | null>(null);
	let busy = $state(false);

	const isCsv = (f: File) => /\.csv$/i.test(f.name);

	async function choose(f: File | null) {
		file = f;
		preview = null;
		error = null;
		columns = [];
		column = '';
		if (!f) return;
		if (isCsv(f)) {
			columns = csvColumns(await f.slice(0, 4096).text());
			column = columns[0] ?? '';
		}
		await check();
	}

	async function check() {
		if (!file) return;
		busy = true;
		error = null;
		try {
			preview = await classificationApi.termsPreview(cls.id, file, column || undefined);
		} catch (err) {
			preview = null;
			error = failure(err);
		} finally {
			busy = false;
		}
	}

	async function upload() {
		if (!file) return;
		busy = true;
		error = null;
		try {
			const done = await classificationApi.termsUpload(cls.id, file, column || undefined);
			onterms({ count: done.count, uploaded_by: cls.terms.uploaded_by, uploaded_at: done.uploaded_at });
			file = null;
			preview = null;
		} catch (err) {
			error = failure(err);
		} finally {
			busy = false;
		}
	}

	async function clear() {
		busy = true;
		error = null;
		try {
			await classificationApi.termsClear(cls.id);
			onterms({ count: 0, uploaded_by: null, uploaded_at: null });
		} catch (err) {
			error = failure(err);
		} finally {
			busy = false;
		}
	}
</script>

<div class="space-y-2">
	<p class="text-sm">
		{cls.terms.count > 0 ? t('cls-terms-count', { count: cls.terms.count }) : t('cls-terms-none')}
		{#if cls.terms.uploaded_at}<span class="text-base-content/60">{t('cls-terms-uploaded', { date: dt(cls.terms.uploaded_at) })}</span>{/if}
	</p>
	{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}
	<div class="flex flex-wrap items-end gap-2">
		<input type="file" class="file-input file-input-sm" aria-label={t('cls-terms-file')} accept=".csv,.txt,text/plain,text/csv" onchange={(e) => void choose(e.currentTarget.files?.[0] ?? null)} />
		{#if columns.length > 1}
			<label class="flex flex-col gap-1">
				<span class="text-xs">{t('cls-terms-column')}</span>
				<select class="select select-sm" bind:value={column} onchange={() => void check()}>
					{#each columns as name (name)}<option value={name}>{name}</option>{/each}
				</select>
			</label>
		{/if}
		{#if cls.terms.count > 0}<button class="btn btn-ghost btn-sm" type="button" disabled={busy} onclick={clear}>{t('cls-terms-clear')}</button>{/if}
	</div>
	{#if preview}
		<div class="space-y-1 text-sm" role="status">
			<p class="m-0">{t('cls-terms-preview', { count: preview.count, duplicates: preview.duplicates })}</p>
			{#if preview.too_short.length > 0}<p class="m-0 text-warning">{t('cls-terms-too-short', { count: preview.too_short.length, sample: preview.too_short.slice(0, 5).join(', ') })}</p>{/if}
			{#if preview.sample.length > 0}<p class="m-0 text-base-content/60">{t('cls-terms-sample', { sample: preview.sample.join(', ') })}</p>{/if}
			<button class="btn btn-primary btn-sm" type="button" disabled={busy} onclick={upload}>{t('cls-terms-upload')}</button>
		</div>
	{/if}
</div>
