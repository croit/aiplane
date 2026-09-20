<script lang="ts">
	import type { AdminLimitsData, AdminLimitRule, LimitSubjectNames } from '$lib/admin-limits';
	import { limitSubjectLabel, limitValue } from '$lib/admin-limits';
	import { n, t } from '$lib/i18n.svelte';

	let { data, onedit, onremove }: { data: AdminLimitsData; onedit: (rule: AdminLimitRule) => void; onremove: (rule: AdminLimitRule) => Promise<void> } = $props();
	const subjectNames: LimitSubjectNames = { global: t('limits-subject-global'), role: t('limits-subject-role'), user: t('limits-subject-user'), token: t('limits-subject-token') };
	function dimensionLabel(rule: AdminLimitRule): string {
		return rule.dimension === 'cost' ? t('limits-dim-cost-short') : t(`limits-dim-${rule.dimension}`);
	}
</script>

<article class="card border border-base-300 bg-base-100"><div class="card-body gap-2 p-4">
	{#if data.limits.length === 0}<p class="m-0 text-sm text-base-content/60">{t('limits-none')}</p>
	{:else}<div class="overflow-x-auto"><table class="table table-sm"><thead><tr><th>{t('limits-col-subject')}</th><th>{t('limits-col-scope')}</th><th>{t('limits-col-limit')}</th><th>{t('limits-col-window')}</th><th class="text-right">{t('limits-col-value')}</th><th class="text-right">{t('limits-col-actions')}</th></tr></thead><tbody>{#each data.limits as rule (rule.id)}<tr><td>{limitSubjectLabel(rule, data, subjectNames)}</td><td class="break-all font-mono">{rule.model ?? t('limits-all-models')}</td><td>{dimensionLabel(rule)}</td><td>{t(`limits-win-${rule.window}`)}</td><td class="text-right tabular-nums">{limitValue(rule, data.currency, n)}</td><td class="text-right"><button type="button" class="btn btn-ghost btn-xs" aria-label={t('limits-edit')} title={t('limits-edit')} onclick={() => onedit(rule)}><svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m4 16-.7 3.2L6.5 18 18 6.5 15.5 4zM14 5.5l2.5 2.5" /></svg></button><button type="button" class="btn btn-ghost btn-xs text-error" aria-label={t('limits-delete')} title={t('limits-delete')} onclick={() => onremove(rule)}><svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6M10 11v5M14 11v5" /></svg></button></td></tr>{/each}</tbody></table></div>{/if}
</div></article>
