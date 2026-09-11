<script lang="ts">
	export type NavIconName =
		| 'chart'
		| 'clock'
		| 'cpu'
		| 'cube'
		| 'database'
		| 'folder'
		| 'key'
		| 'message'
		| 'plug'
		| 'send'
		| 'sliders'
		| 'sparkles'
		| 'users';

	let { name, size = 16 } = $props<{ name: NavIconName; size?: number }>();
</script>

<svg
	xmlns="http://www.w3.org/2000/svg"
	viewBox="0 0 24 24"
	width={size}
	height={size}
	fill="none"
	stroke="currentColor"
	stroke-width="1.75"
	stroke-linecap="round"
	stroke-linejoin="round"
	aria-hidden="true"
>
	{#if name === 'chart'}
		<path d="M3 3v18h18"/><path d="M18 17V9"/><path d="M13 17V5"/><path d="M8 17v-3"/>
	{:else if name === 'clock'}
		<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>
	{:else if name === 'cpu'}
		<rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6"/><path d="M9 1v3M15 1v3M9 20v3M15 20v3M20 9h3M20 14h3M1 9h3M1 14h3"/>
	{:else if name === 'cube'}
		<path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><path d="M3.27 6.96 12 12.01l8.73-5.05M12 22.08V12"/>
	{:else if name === 'database'}
		<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5v14a9 3 0 0 0 18 0V5M3 12a9 3 0 0 0 18 0"/>
	{:else if name === 'folder'}
		<path d="M20 20H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2z"/>
	{:else if name === 'key'}
		<circle cx="7.5" cy="15.5" r="3.5"/><path d="m10 13 9-9M17 6l3 3M14 9l3 3"/>
	{:else if name === 'message'}
		<path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8z"/>
	{:else if name === 'plug'}
		<path d="M12 22v-5M9 8V2M15 8V2M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z"/>
	{:else if name === 'send'}
		<path d="M12 19V5M5 12l7-7 7 7"/>
	{:else if name === 'sliders'}
		<path d="M4 6h7M15 6h5M4 12h2M10 12h10M4 18h9M17 18h3"/><circle cx="13" cy="6" r="2"/><circle cx="8" cy="12" r="2"/><circle cx="15" cy="18" r="2"/>
	{:else if name === 'sparkles'}
		<path d="M12 3l1.9 5.8a2 2 0 0 0 1.3 1.3L21 12l-5.8 1.9a2 2 0 0 0-1.3 1.3L12 21l-1.9-5.8a2 2 0 0 0-1.3-1.3L3 12l5.8-1.9a2 2 0 0 0 1.3-1.3zM5 3v4M19 17v4M3 5h4M17 19h4"/>
	{:else}
		<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75"/>
	{/if}
</svg>
