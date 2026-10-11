export type Priority = 'low' | 'medium' | 'high';

export function canChoosePriority(priority: Priority | null, maximum: Priority): boolean {
	const priorities: Priority[] = ['low', 'medium', 'high'];
	return priority === null || priorities.indexOf(priority) <= priorities.indexOf(maximum);
}
export interface PriorityMapping { field: string; low: number; medium: number; high: number }
export type BackendPriority = { mode: 'auto' | 'gateway' } | { mode: 'custom'; mapping: PriorityMapping };
