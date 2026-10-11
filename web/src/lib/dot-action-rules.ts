import type { DotAccessSelection, DotAccessTool, DotActionRule } from './dots.ts';

export interface DotActionRow {
	id: string;
	description: string | null;
	available: boolean;
	rule: string | null;
}

export function actionRows(tool: DotAccessTool): DotActionRow[] {
	const catalogue = tool.actions ?? [];
	return [
		...catalogue.map((action) => ({ ...action, available: true, rule: tool.action_rules.find((rule) => rule.id === action.id)?.rule ?? null })),
		...tool.action_rules.filter((rule) => !catalogue.some((action) => action.id === rule.id))
			.map((rule) => ({ ...rule, available: false, description: null }))
	];
}

export function setActionRule(tool: DotAccessTool, id: string, rule: DotActionRule | null): DotAccessTool {
	const remaining = tool.action_rules.filter((saved) => saved.id !== id);
	return { ...tool, action_rules: rule === null ? remaining : [...remaining, { id, rule }] };
}

function validRule(rule: string): rule is DotActionRule {
	return rule === 'allow' || rule === 'ask' || rule === 'deny';
}

export function accessSelections(tools: DotAccessTool[]): DotAccessSelection[] | null {
	const selected = tools.filter((tool) => tool.selected);
	if (selected.some((tool) => tool.action_rules.some((rule) => !validRule(rule.rule)
		|| !tool.actions?.some((action) => action.id === rule.id)))) return null;
	return selected.map((tool) => ({
		id: tool.id, permission: tool.permission,
		actions: tool.action_rules.map((rule) => ({ id: rule.id, rule: rule.rule as DotActionRule }))
	}));
}
