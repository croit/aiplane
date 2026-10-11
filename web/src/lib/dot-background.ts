import type { Dot } from './dot-state.ts';
import type { SearchOption } from './searchable-select.ts';

export interface DotBinding {
	dot_id: string | null;
	dot: Pick<Dot, 'id' | 'name' | 'display' | 'active'> | null;
}

export function dotRunOptions(
	dots: readonly Dot[],
	initialId: string | null,
	labels: { person: string; inactive: string; unavailable: (id: string) => string }
): SearchOption[] {
	const options: SearchOption[] = [{ value: '', label: labels.person }];
	for (const dot of dots) {
		options.push({
			value: dot.id,
			label: dot.display || dot.name,
			description: dot.active ? dot.description : labels.inactive,
			keywords: [dot.name, dot.id],
			disabled: !dot.active && dot.id !== initialId
		});
	}
	if (initialId && !dots.some((dot) => dot.id === initialId)) {
		options.push({ value: initialId, label: labels.unavailable(initialId), disabled: false });
	}
	return options;
}
