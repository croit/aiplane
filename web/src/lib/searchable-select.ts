export interface SearchOptionBadge {
	label: string;
	tone: 'success' | 'error';
}

export interface SearchOption {
	value: string;
	label: string;
	description?: string;
	badges?: SearchOptionBadge[];
	keywords?: string[];
	disabled?: boolean;
}

function searchableText(option: SearchOption): string {
	return [option.label, option.value, option.description, ...(option.badges ?? []).map((badge) => badge.label), ...(option.keywords ?? [])]
		.filter(Boolean)
		.join(' ')
		.normalize('NFKD')
		.replace(/\p{Diacritic}/gu, '')
		.toLocaleLowerCase();
}

export function filterSearchOptions(options: SearchOption[], query: string): SearchOption[] {
	const terms = query
		.normalize('NFKD')
		.replace(/\p{Diacritic}/gu, '')
		.toLocaleLowerCase()
		.trim()
		.split(/\s+/)
		.filter(Boolean);
	if (terms.length === 0) return options;
	return options.filter((option) => {
		const haystack = searchableText(option);
		return terms.every((term) => haystack.includes(term));
	});
}

export function nextEnabledOptionIndex(options: SearchOption[], current: number, direction: 1 | -1): number {
	if (options.length === 0 || options.every((option) => option.disabled)) return -1;
	let index = current;
	for (let visited = 0; visited < options.length; visited += 1) {
		index = (index + direction + options.length) % options.length;
		if (!options[index]?.disabled) return index;
	}
	return -1;
}
