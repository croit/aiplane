<script lang="ts">
	import { levelBadgeClass, type LevelColor } from '#lib/classification.js';

	let { name, color, title = undefined }: { name: string; color: LevelColor; title?: string } = $props();
</script>

<span class={levelBadgeClass(color)} {title}>{name}</span>
