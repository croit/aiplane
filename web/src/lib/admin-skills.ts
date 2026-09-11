export interface AdminSkill {
	name: string;
	title: string;
	description: string;
	files: string[];
	body: string | null;
	all_skills_groups: string[];
	granted_groups: string[];
}

export interface AdminSkillsData {
	skills: AdminSkill[];
	groups: string[];
	configured: boolean;
	directory_accessible: boolean;
	source: string | null;
}

export function selectedAdminSkill(skills: AdminSkill[], requested: string | null): AdminSkill | null {
	return skills.find((skill) => skill.name === requested) ?? skills[0] ?? null;
}

export function effectiveSkillGroups(skill: AdminSkill): string[] {
	return [...new Set([...skill.all_skills_groups, ...skill.granted_groups])];
}

export interface SkillFileGroup {
	directory: string | null;
	files: string[];
}

export function skillFileTree(files: string[]): SkillFileGroup[] {
	const groups = new Map<string | null, string[]>();
	for (const path of files) {
		const separator = path.indexOf('/');
		const directory = separator === -1 ? null : path.slice(0, separator);
		const name = separator === -1 ? path : path.slice(separator + 1);
		groups.set(directory, [...(groups.get(directory) ?? []), name]);
	}
	return [...groups].map(([directory, groupedFiles]) => ({ directory, files: groupedFiles }));
}
