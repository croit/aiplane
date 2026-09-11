export type LimitSubjectType = 'global' | 'role' | 'user' | 'token';
export type LimitDimension = 'requests' | 'tokens' | 'cost';
export type LimitWindow = 'hour' | 'day' | 'week' | 'month';

export interface AdminLimitRule {
	id: string;
	subject_type: LimitSubjectType;
	subject_id: string;
	model: string | null;
	dimension: LimitDimension;
	window: LimitWindow;
	value: number;
	managed_by: string;
}

export interface AdminLimitsData {
	limits: AdminLimitRule[];
	users: { id: string; email: string }[];
	tokens: { id: string; name: string; owner: string }[];
	roles: string[];
	models: string[];
	currency: string;
}

export interface LimitSubjectNames {
	global: string;
	role: string;
	user: string;
	token: string;
}

export function limitSubjectName(subject: Pick<AdminLimitRule, 'subject_type' | 'subject_id'>, data: Pick<AdminLimitsData, 'users' | 'tokens'>): string {
	if (subject.subject_type === 'user') return data.users.find((user) => user.id === subject.subject_id)?.email ?? subject.subject_id;
	if (subject.subject_type === 'token') {
		const token = data.tokens.find((candidate) => candidate.id === subject.subject_id);
		return token ? `${token.name} (${token.owner})` : subject.subject_id;
	}
	return subject.subject_id;
}

export function limitSubjectLabel(subject: Pick<AdminLimitRule, 'subject_type' | 'subject_id'>, data: Pick<AdminLimitsData, 'users' | 'tokens'>, names: LimitSubjectNames): string {
	if (subject.subject_type === 'global') return names.global;
	return `${names[subject.subject_type]}: ${limitSubjectName(subject, data)}`;
}

export function limitValue(rule: Pick<AdminLimitRule, 'dimension' | 'value'>, currency: string, format: (value: number, options?: Intl.NumberFormatOptions) => string): string {
	if (rule.dimension === 'cost') return `${format(rule.value, { minimumFractionDigits: 2, maximumFractionDigits: 2 })} ${currency}`;
	return format(Math.trunc(rule.value), { maximumFractionDigits: 0 });
}
