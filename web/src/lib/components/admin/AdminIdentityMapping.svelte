<script lang="ts">
	/**
	 * What the identity provider actually sends, and where it lands.
	 *
	 * Both failure directions are silent without this: a claim value that maps to
	 * nothing leaves the user with only the default group's access, and a mapping
	 * no login has ever matched is usually a value mistyped when the group was
	 * created. Neither shows up anywhere else.
	 */
	import { identityRows } from '$lib/admin-groups';
	import { t } from '$lib/i18n.svelte';
	import type { AdminGroup } from './AdminGroupForm.svelte';

	let {
		observed,
		groups,
		oncreate
	}: {
		observed: string[];
		groups: AdminGroup[];
		oncreate: (value: string) => void;
	} = $props();

	let rows = $derived(identityRows(observed, groups));
</script>

<section class="flex flex-col gap-3">
	<header class="flex flex-col gap-1">
		<h2 class="text-lg font-semibold">{t('groups-identity-heading')}</h2>
		<p class="max-w-3xl text-sm text-base-content/70">{t('groups-identity-intro')}</p>
	</header>

	{#if rows.length === 0}
		<div class="alert"><span>{t('groups-identity-empty')}</span></div>
	{:else}
		<article class="card border border-base-300 bg-base-100">
			<div class="card-body gap-0 overflow-x-auto">
				<table class="table table-sm">
					<thead>
						<tr>
							<th>{t('groups-identity-col-value')}</th>
							<th>{t('groups-identity-col-groups')}</th>
						</tr>
					</thead>
					<tbody>
						{#each rows as row (row.value)}
							<tr>
								<td class="font-mono text-xs">{row.value}</td>
								<td>
									{#if row.groups.length === 0}
										<div class="flex flex-wrap items-center gap-2">
											<span class="badge badge-warning badge-sm">{t('groups-identity-unmapped')}</span>
											<button type="button" class="btn btn-ghost btn-xs" onclick={() => oncreate(row.value)}>{t('groups-identity-create')}</button>
										</div>
										<span class="mt-1 block text-xs text-base-content/60">{t('groups-identity-unmapped-hint')}</span>
									{:else}
										<div class="flex flex-wrap items-center gap-1">
											{#each row.groups as name (name)}
												<span class="badge badge-outline badge-sm font-mono">{name}</span>
											{/each}
											{#if !row.seen}
												<span class="badge badge-ghost badge-sm">{t('groups-identity-unseen')}</span>
											{/if}
										</div>
										{#if !row.seen}
											<span class="mt-1 block text-xs text-base-content/60">{t('groups-identity-unseen-hint')}</span>
										{/if}
									{/if}
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</article>
	{/if}
</section>
