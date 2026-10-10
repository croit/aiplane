export interface Dot {
	id: string;
	name: string;
	display: string;
	description: string;
	active: boolean;
}

export function activeDotCount(dots: readonly Dot[]): number {
	return dots.reduce((count, dot) => count + Number(dot.active), 0);
}

export function canCreateDot(active: number, limit: number): boolean {
	return active < limit;
}
