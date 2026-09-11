import type { SearchOption } from './searchable-select';

export interface ChatModelOption {
	id: string;
	gdpr: boolean;
	nda: boolean;
}

export function modelSelectOptions(
	models: ChatModelOption[],
	labels: { gdpr: string; nda: string }
): SearchOption[] {
	return models.map((model) => ({
		value: model.id,
		label: model.id,
		badges: [
			{ label: labels.gdpr, tone: model.gdpr ? 'success' : 'error' },
			{ label: labels.nda, tone: model.nda ? 'success' : 'error' }
		]
	}));
}
