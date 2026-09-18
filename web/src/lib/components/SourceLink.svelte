<script lang="ts">
	import { t } from '$lib/i18n.svelte';

	let { login = false } = $props<{ login?: boolean }>();
	let sourceUrl = $state('https://github.com/croit/aiplane');
	let version = $state('v0.1.0');

	$effect(() => {
		fetch('/api/v0/build', { headers: { accept: 'application/json' } })
			.then(async (response) => {
				if (!response.ok) return;
				const build = (await response.json()) as { source_url: string; version: string };
				sourceUrl = build.source_url;
				version = build.version;
			})
			.catch(() => {});
	});
</script>

<a
	href={sourceUrl}
	class="link link-hover"
	target="_blank"
	rel="noopener noreferrer"
	title={t('nav-source-title')}
>
	{login ? t('login-source-link') : t('nav-source-line', { version })}
</a>
