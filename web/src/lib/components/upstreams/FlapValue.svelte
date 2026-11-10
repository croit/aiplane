<script lang="ts">
	import { untrack } from 'svelte';

	/**
	 * The board's signature move: a value that changes cascades through its
	 * characters, each flipping in place. Under 16 ms per character so a
	 * three-digit change finishes before the next tick lands. Respects
	 * prefers-reduced-motion (the value simply swaps).
	 */
	let { value }: { value: string } = $props();

	let chars = $state<string[]>(untrack(() => value.split('')));
	let previous = $state(untrack(() => value));
	let flipping = $state<number[]>([]);
	let reduced = $state(false);

	$effect(() => {
		reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
	});

	$effect(() => {
		value;
		if (value === previous) return;
		if (reduced || value.length !== previous.length) {
			chars = value.split('');
			previous = value;
			return;
		}
		const changed: number[] = [];
		for (let i = 0; i < value.length; i++) {
			if (value[i] !== chars[i]) changed.push(i);
		}
		flipping = changed;
		const timer = setTimeout(() => {
			chars = value.split('');
			flipping = [];
			previous = value;
		}, 160);
		return () => clearTimeout(timer);
	});
</script>

<span class="flap-value" aria-label={value}>
	{#each chars as char, i (i)}
		<span class="flap-char {flipping.includes(i) ? 'flipping' : ''}">{char}</span>
	{/each}
</span>

<style>
	.flap-value {
		display: inline-block;
		font-variant-numeric: tabular-nums;
	}
	.flap-char {
		display: inline-block;
		min-width: 0.55em;
		text-align: center;
	}
	.flap-char.flipping {
		animation: flap 160ms ease-in-out;
	}
	@keyframes flap {
		0% {
			transform: rotateX(0);
		}
		50% {
			transform: rotateX(88deg);
			opacity: 0.4;
		}
		100% {
			transform: rotateX(0);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.flap-char.flipping {
			animation: none;
		}
	}
</style>
