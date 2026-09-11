<script lang="ts">
	import type { FeatureDefault } from '$lib/admin-models';
	import { t } from '$lib/i18n.svelte';
	import SearchableSelect from '$lib/components/SearchableSelect.svelte';

	let { defaults, onsave }: {
		defaults: FeatureDefault[];
		onsave: (feature: string, model: string) => Promise<void>;
	} = $props();

	const labels: Record<string, string> = {
		chat: 'admin-defaults-chat-label',
		transcription: 'admin-defaults-voice-label',
		image: 'admin-defaults-image-label',
		embedding: 'admin-defaults-embedding-label'
	};
</script>

{#if defaults.length > 0}
	<article class="card border border-base-300 bg-base-100">
		<div class="card-body gap-3">
			<header class="flex flex-col gap-1">
				<h2 class="card-title text-base">{t('admin-defaults-heading')}</h2>
				<p class="text-sm text-base-content/70">{t('admin-defaults-intro')}</p>
			</header>
			<div class="grid grid-cols-1 gap-4 md:grid-cols-2">
				{#each defaults as entry (entry.feature)}
					<label class="flex flex-col gap-1">
						<span class="label-text mb-1 text-xs">{t(labels[entry.feature] ?? entry.feature)}</span>
						<SearchableSelect
							options={[{ value: '', label: t('admin-defaults-first-option') }, ...entry.available.map((model) => ({ value: model, label: model }))]}
							value={entry.model ?? ''}
							onchange={(value) => onsave(entry.feature, value)}
							ariaLabel={t(labels[entry.feature] ?? entry.feature)}
							size="sm"
							class="w-full"
						/>
					</label>
				{/each}
			</div>
		</div>
	</article>
{/if}
