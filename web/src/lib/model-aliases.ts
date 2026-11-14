/** One alias target as the server resolved it. */
export interface ModelAliasTarget {
	backend: string;
	/** `null` binds to the backend's sole model. */
	target: string | null;
	/** The model a request would reach right now; `null` when nothing matches. */
	resolves_to: string | null;
	served: boolean;
}

export interface ModelAlias {
	name: string;
	targets: ModelAliasTarget[];
}

/** A backend and the real models it serves now, for the target picker. */
export interface AliasBackend {
	name: string;
	models: string[];
}

export interface ModelAliasesData {
	aliases: ModelAlias[];
	backends: AliasBackend[];
}

/** The editor's shape: `''` is the "sole model" choice. */
export interface AliasDraftTarget {
	backend: string;
	target: string;
}

export interface AliasDraft {
	name: string;
	targets: AliasDraftTarget[];
}

/** The problem that prevents saving the name, as an i18n key, or `null`. */
export function aliasNameProblem(name: string, realModels: readonly string[]): string | null {
	const trimmed = name.trim();
	if (!trimmed) return 'model-alias-name-required';
	if (/\s/.test(trimmed) || /[\u0000-\u001f\u007f]/.test(trimmed)) {
		return 'model-alias-name-invalid';
	}
	if (realModels.includes(trimmed)) return 'model-alias-name-taken';
	return null;
}

/** The problem that prevents saving the targets, as an i18n key, or `null`. */
export function aliasTargetProblem(targets: readonly AliasDraftTarget[]): string | null {
	if (targets.length === 0) return 'model-alias-targets-required';
	const seen = new Set<string>();
	for (const target of targets) {
		if (!target.backend) return 'model-alias-target-missing';
		if (seen.has(target.backend)) return 'model-alias-target-duplicate';
		seen.add(target.backend);
	}
	return null;
}

export function draftFromAlias(alias: ModelAlias): AliasDraft {
	return {
		name: alias.name,
		targets: alias.targets.map((target) => ({
			backend: target.backend,
			target: target.target ?? ''
		}))
	};
}

/** How many targets resolve right now. */
export function servedTargetCount(alias: ModelAlias): number {
	return alias.targets.filter((target) => target.served).length;
}
