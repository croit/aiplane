<script lang="ts">
	import type { FeatureDefault } from '$lib/admin-models';
	import { t } from '$lib/i18n.svelte';

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
						<select class="select select-bordered select-sm w-full" aria-label={t(labels[entry.feature] ?? entry.feature)} value={entry.model ?? ''} onchange={(event) => onsave(entry.feature, event.currentTarget.value)}>
							<option value="">{t('admin-defaults-first-option')}</option>
							{#each entry.available as model}<option value={model}>{model}</option>{/each}
						</select>
					</label>
				{/each}
			</div>
		</div>
	</article>
{/if}
