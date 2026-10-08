<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { t } from '#lib/i18n.svelte.js';
	import { GUIDE_TABS, selectedGuideTab } from '#lib/tokens-tabs.js';

	let origin = $state('https://llm.croit.io');
	let client = $derived(selectedGuideTab(page.url.search));

	onMount(() => { origin = window.location.origin; });

	const labels = {
		opencode: 'tokens-guide-opencode',
		claude: 'tokens-guide-claude',
		pi: 'tokens-guide-pi',
		omp: 'tokens-guide-omp',
		python: 'tokens-guide-python'
	} as const;

	let opencodeConfig = $derived(JSON.stringify({
		$schema: 'https://opencode.ai/config.json',
		provider: {
			aiplane: {
				npm: '@ai-sdk/openai-compatible',
				name: 'croit AIplane',
				options: { baseURL: `${origin}/v1` },
				models: { YOUR_MODEL_ID: { name: 'YOUR_MODEL_ID' } }
			}
		}
	}, null, 2));
	let claudeConfig = $derived(`export ANTHROPIC_BASE_URL=${origin}\nexport ANTHROPIC_AUTH_TOKEN=gwk_PASTE_YOUR_TOKEN\nexport ANTHROPIC_MODEL=YOUR_MODEL_ID\nexport ANTHROPIC_DEFAULT_HAIKU_MODEL=YOUR_MODEL_ID\nclaude`);
	let piConfig = $derived(JSON.stringify({
		providers: {
			aiplane: {
				baseUrl: `${origin}/v1`,
				api: 'openai-completions',
				apiKey: 'gwk_PASTE_YOUR_TOKEN',
				models: [{ id: 'YOUR_MODEL_ID' }]
			}
		}
	}, null, 2));
	let ompConfig = $derived(`providers:\n  aiplane:\n    baseUrl: ${origin}/v1\n    api: openai-completions\n    apiKey: gwk_PASTE_YOUR_TOKEN\n    models:\n      - id: YOUR_MODEL_ID\n        contextWindow: 128000\n        maxTokens: 16384`);
	const pythonShell = 'python -m pip install openai\nexport OPENAI_API_KEY=gwk_PASTE_YOUR_TOKEN';
	let pythonConfig = $derived(`from openai import OpenAI\n\nclient = OpenAI(base_url="${origin}/v1")\nresponse = client.chat.completions.create(\n    model="YOUR_MODEL_ID",\n    messages=[{"role": "user", "content": "Hello!"}],\n)\nprint(response.choices[0].message.content)`);
</script>

<section class="flex flex-col gap-4">
	<div>
		<h2 class="text-lg font-semibold">{t('tokens-guides-heading')}</h2>
		<p class="max-w-3xl text-sm text-base-content/70">{t('tokens-guides-intro')}</p>
	</div>
	<div class="alert alert-info"><span>{t('tokens-guides-before')}</span></div>
	<nav class="tabs tabs-box max-w-full overflow-x-auto" aria-label={t('tokens-guides-heading')}>
		{#each GUIDE_TABS as guide}
			<a href="/settings/tokens?tab=guides&client={guide}" class="tab whitespace-nowrap {client === guide ? 'tab-active' : ''}" aria-current={client === guide ? 'page' : undefined}>{t(labels[guide])}</a>
		{/each}
	</nav>

	{#if client === 'opencode'}
		<article class="card"><div class="card-body gap-4">
			<h3 class="card-title">{t('tokens-guide-opencode')}</h3>
			<ol class="list-decimal space-y-3 pl-5 text-sm">
				<li>{t('tokens-opencode-step-1')}</li>
				<li>{t('tokens-opencode-step-2')}</li>
				<li>{t('tokens-opencode-step-3')}</li>
			</ol>
			<pre class="w-full overflow-x-auto rounded-box bg-base-200 p-4 text-xs"><code>{opencodeConfig}</code></pre>
			<p class="text-sm text-base-content/70">{t('tokens-opencode-finish')}</p>
		</div></article>
	{:else if client === 'claude'}
		<article class="card"><div class="card-body gap-4">
			<h3 class="card-title">{t('tokens-guide-claude')}</h3>
			<ol class="list-decimal space-y-3 pl-5 text-sm">
				<li>{t('tokens-claude-step-1')}</li>
				<li>{t('tokens-claude-step-2')}</li>
				<li>{t('tokens-claude-step-3')}</li>
			</ol>
			<pre class="w-full overflow-x-auto rounded-box bg-base-200 p-4 text-xs"><code>{claudeConfig}</code></pre>
			<p class="text-sm text-base-content/70">{t('tokens-claude-finish')}</p>
		</div></article>
	{:else if client === 'pi'}
		<article class="card"><div class="card-body gap-4">
			<h3 class="card-title">{t('tokens-guide-pi')}</h3>
			<ol class="list-decimal space-y-3 pl-5 text-sm">
				<li>{t('tokens-pi-step-1')}</li>
				<li>{t('tokens-pi-step-2')}</li>
			</ol>
			<pre class="w-full overflow-x-auto rounded-box bg-base-200 p-4 text-xs"><code>{piConfig}</code></pre>
			<p class="text-sm text-base-content/70">{t('tokens-pi-finish')}</p>
		</div></article>
	{:else if client === 'omp'}
		<article class="card"><div class="card-body gap-4">
			<h3 class="card-title">{t('tokens-guide-omp')}</h3>
			<ol class="list-decimal space-y-3 pl-5 text-sm">
				<li>{t('tokens-omp-step-1')}</li>
				<li>{t('tokens-omp-step-2')}</li>
			</ol>
			<pre class="w-full overflow-x-auto rounded-box bg-base-200 p-4 text-xs"><code>{ompConfig}</code></pre>
			<p class="text-sm text-base-content/70">{t('tokens-omp-finish')}</p>
		</div></article>
	{:else}
		<article class="card"><div class="card-body gap-4">
			<h3 class="card-title">{t('tokens-guide-python')}</h3>
			<ol class="list-decimal space-y-3 pl-5 text-sm">
				<li>{t('tokens-python-step-1')}</li>
				<li>{t('tokens-python-step-2')}</li>
				<li>{t('tokens-python-step-3')}</li>
			</ol>
			<pre class="w-full overflow-x-auto rounded-box bg-base-200 p-4 text-xs"><code>{pythonShell}</code></pre>
			<pre class="w-full overflow-x-auto rounded-box bg-base-200 p-4 text-xs"><code>{pythonConfig}</code></pre>
			<p class="text-sm text-base-content/70">{t('tokens-python-finish')}</p>
		</div></article>
	{/if}
	<p class="text-sm text-base-content/70">{t('tokens-guides-model-note')}</p>
</section>
