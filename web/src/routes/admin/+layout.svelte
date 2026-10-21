<script lang="ts">
	let { children }: { children: import('svelte').Snippet } = $props();
	import { me } from '#lib/session.svelte.js';
	import { t } from '#lib/i18n.svelte.js';


</script>

{#if !me.value?.role_ids?.includes('admin')}
	<div class="alert alert-warning mb-4"><span>{t('admin-needs-admin-role')}</span></div>
{/if}

{@render children()}
