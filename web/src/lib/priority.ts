export type Priority = 'low' | 'medium' | 'high';
export interface PriorityMapping { field: string; low: number; medium: number; high: number }
export type BackendPriority = { mode: 'auto' | 'gateway' } | { mode: 'custom'; mapping: PriorityMapping };
