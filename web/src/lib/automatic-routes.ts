export type NewRouteBlocker = 'selector' | 'candidates' | null;

export function editableRoute<Candidate extends object, Route extends { candidates: Candidate[] }>(route: Route): Route {
	return {
		...route,
		candidates: route.candidates.map((candidate) => ({ ...candidate }))
	};
}

export function newRouteBlocker(selectorModels: string[], candidateModels: string[]): NewRouteBlocker {
	if (selectorModels.length === 0) return 'selector';
	if (new Set(candidateModels.filter((model) => model.trim())).size < 2) return 'candidates';
	return null;
}
