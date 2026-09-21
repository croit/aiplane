import type { SearchOption } from './searchable-select.ts';

/**
 * Selection logic for the multi-value grant pickers, kept out of the component
 * so it runs under `node --test` (the SPA suite has no DOM).
 */

/** The one grant value that is not an id: "everything, including what ships later". */
export const GRANT_WILDCARD = '*';

export interface MultiSelectLabels {
	/** Shown on the wildcard row. Omit where the field has no wildcard grant. */
	wildcardLabel?: string;
	/** Shown on rows the wildcard already covers. */
	shadowLabel?: string;
	/** Shown on a held value that matches no option. */
	unknownLabel?: string;
}

export interface SummaryLabels {
	empty: string;
	counted: (count: number) => string;
	wildcard?: string;
}

export function isWildcardSelected(selected: readonly string[]): boolean {
	return selected.includes(GRANT_WILDCARD);
}

export function normalizeSelection(values: readonly string[]): string[] {
	const out: string[] = [];
	for (const raw of values) {
		const value = raw.trim();
		if (!value || out.includes(value)) continue;
		out.push(value);
	}
	return out.includes(GRANT_WILDCARD) ? [GRANT_WILDCARD] : out;
}

/**
 * The wildcard and concrete ids are mutually exclusive rather than additive.
 * Storing `["*", "search_web"]` would resolve identically to `["*"]` but read as
 * a narrower grant than it is, so whichever the operator picked last wins.
 */
export function toggleValue(selected: readonly string[], value: string, on: boolean): string[] {
	if (!on) return normalizeSelection(selected.filter((held) => held !== value));
	if (value === GRANT_WILDCARD) return [GRANT_WILDCARD];
	return normalizeSelection([...selected.filter((held) => held !== GRANT_WILDCARD), value]);
}

/**
 * Held values that no option offers — a tool a release removed, a group somebody
 * deleted. The form surfaces them so saving cannot quietly drop a grant nobody
 * meant to revoke.
 */
export function unknownValues(selected: readonly string[], options: readonly SearchOption[]): string[] {
	const known = new Set(options.map((option) => option.value));
	return selected.filter((value) => value !== GRANT_WILDCARD && !known.has(value));
}

/**
 * The rows to render: the wildcard first where the field has one, then the known
 * options, then any held value that matches none of them. Rows the wildcard
 * already covers are disabled rather than hidden, so the grant stays legible.
 */
export function multiSelectOptions(
	options: readonly SearchOption[],
	labels: MultiSelectLabels,
	selected: readonly string[]
): SearchOption[] {
	const shadowed = labels.wildcardLabel !== undefined && isWildcardSelected(selected);
	const rows: SearchOption[] = [];
	if (labels.wildcardLabel !== undefined) {
		rows.push({ value: GRANT_WILDCARD, label: labels.wildcardLabel });
	}
	for (const option of options) {
		rows.push(shadowed ? { ...option, disabled: true, description: labels.shadowLabel } : option);
	}
	for (const value of unknownValues(selected, options)) {
		rows.push({ value, label: value, description: labels.unknownLabel });
	}
	return rows;
}

export function summarizeSelection(
	selected: readonly string[],
	options: readonly SearchOption[],
	labels: SummaryLabels
): string {
	if (isWildcardSelected(selected) && labels.wildcard !== undefined) return labels.wildcard;
	if (selected.length === 0) return labels.empty;
	if (selected.length === 1) {
		const only = selected[0] as string;
		return options.find((option) => option.value === only)?.label ?? only;
	}
	return labels.counted(selected.length);
}
