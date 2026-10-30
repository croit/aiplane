/**
 * The data classification scheme: the types of `/api/v0/admin/classification`,
 * the calls, and the pure logic the assistant and the overview editor share.
 *
 * The editor works on a {@link Draft}, a deep copy of what the server sent;
 * nothing is saved until one `PUT` at the end. Everything here is framework
 * free so it can be unit-tested without a DOM.
 */
import { adminJson, adminPut } from './admin-client.ts';
import { ApiError, request, responseError } from './api.ts';

export type LevelColor = 'neutral' | 'info' | 'success' | 'warning' | 'error';
export const LEVEL_COLORS: readonly LevelColor[] = ['neutral', 'info', 'success', 'warning', 'error'];

export interface Level {
	id: string;
	rank: number;
	name: string;
	description: string;
	color: LevelColor;
	marking_text: string;
	mip_label_id: string;
	filename_tag: string;
}

export type Action = 'raise' | 'confirm' | 'block';
export const ACTIONS: readonly Action[] = ['raise', 'confirm', 'block'];
export type PatternId = 'email' | 'iban' | 'payment_card' | 'de_tax_id';
export const PATTERN_IDS: readonly PatternId[] = ['email', 'iban', 'payment_card', 'de_tax_id'];

export interface Terms {
	count: number;
	uploaded_by: string | null;
	uploaded_at: string | null;
}

export interface DataClass {
	id: string;
	name: string;
	description: string;
	level_id: string;
	patterns: PatternId[];
	rule_question: string;
	rule_positive: string;
	rule_negative: string;
	rule_threshold: number;
	action: Action;
	terms: Terms;
}

export type MarkingKind = 'mip' | 'tscp' | 'sensitivity' | 'stamp';
export interface Marking {
	kind: MarkingKind;
	value: string;
	level_id: string;
}

export type DestinationKind = 'pool' | 'tool' | 'mcp' | 'connector' | 'collection';
export const DESTINATION_KINDS: readonly DestinationKind[] = ['pool', 'tool', 'mcp', 'connector', 'collection'];
export type Over = 'hide' | 'gate';

export type Hosting = 'own_network' | 'cloud' | 'unknown';

export interface Destination {
	kind: DestinationKind;
	id: string;
	name: string;
	description: string;
	/** `null`: unrated, the lowest level applies. */
	level_id: string | null;
	/** Only meaningful for a tool, MCP server or connector with `egress`. */
	over: Over;
	egress: boolean;
	/** Pools only: where the backends run. */
	hosting?: Hosting;
	/** Pools only: the backend hosts. */
	hosts?: string[];
}

export interface Policy {
	default_level: string;
	lowering: 'never' | 'with_reason';
	gate_on_match: 'ask' | 'block';
	user_override: boolean;
	link_share_max_level: string;
	unknown_label_level: string;
	unreadable_level: string;
	system_one_model: string;
	mode: 'monitor' | 'enforce';
	mip_tenant_id: string;
	output: { visible: boolean; metadata: boolean; filename: boolean };
}

export type ClassWithoutTerms = Omit<DataClass, 'terms'>;

export interface Template {
	id: 'iso' | 'tisax' | 'bsi' | 'tlp';
	name: string;
	levels: Level[];
	classes: ClassWithoutTerms[];
	markings: Marking[];
	policy: Partial<Policy>;
}

export interface SystemOneModel {
	id: string;
	level_id: string | null;
	cleared_for_highest: boolean;
	pools: string[];
}

/** `GET /api/v0/admin/classification`. */
export interface ClassificationState {
	active: boolean;
	levels: Level[];
	classes: DataClass[];
	markings: Marking[];
	destinations: Destination[];
	policy: Policy;
	templates: Template[];
	system_one_models: SystemOneModel[];
}

export interface Assignment {
	kind: DestinationKind;
	id: string;
	level_id: string;
	over: Over;
}

/** The `PUT` body. */
export interface ClassificationBody {
	levels: Level[];
	classes: ClassWithoutTerms[];
	markings: Marking[];
	assignments: Assignment[];
	policy: Policy;
}

export interface TermsPreview {
	count: number;
	duplicates: number;
	too_short: string[];
	sample: string[];
}
export interface TermsUpload extends TermsPreview {
	uploaded_at: string;
}

export interface InspectResult {
	markings: { kind: MarkingKind; value: string; name?: string }[];
	unreadable: boolean;
}

export interface TestResult {
	level_id: string;
	causes: string[];
	classes: string[];
}

/** A conversation's classification, as `GET /chat/sessions/{id}` and the level call return it. */
export interface ChatLevel {
	id: string;
	rank: number;
	name: string;
	color: LevelColor;
}
export interface ChatClassification {
	active: boolean;
	level: ChatLevel | null;
	levels: ChatLevel[];
	lowering?: 'never' | 'with_reason';
	/** The latest event's cause, content-free. */
	cause?: string | null;
}

export interface ChatShares {
	link_allowed: boolean;
	link: boolean;
	subjects: { kind: 'user' | 'group'; id: string; name: string }[];
}

const BASE = '/api/v0/admin/classification';

async function upload<T>(path: string, form: FormData): Promise<T> {
	const res = await fetch(path, { method: 'POST', body: form, credentials: 'same-origin', headers: { accept: 'application/json' } });
	if (!res.ok) {
		const err = await responseError(res);
		throw new Error(err.serverMessage ?? (err.detail || err.message));
	}
	return (await res.json()) as T;
}

export const classificationApi = {
	get: () => adminJson<ClassificationState>(BASE),
	save: (body: ClassificationBody) => adminPut<ClassificationState>(BASE, body),
	termsPreview: (classId: string, file: File, column?: string) =>
		upload<TermsPreview>(`${BASE}/classes/${encodeURIComponent(classId)}/terms?preview=1${column ? `&column=${encodeURIComponent(column)}` : ''}`, fileForm(file)),
	termsUpload: (classId: string, file: File, column?: string) =>
		upload<TermsUpload>(`${BASE}/classes/${encodeURIComponent(classId)}/terms${column ? `?column=${encodeURIComponent(column)}` : ''}`, fileForm(file)),
	termsClear: (classId: string) => adminJson<void>(`${BASE}/classes/${encodeURIComponent(classId)}/terms`, { method: 'DELETE' }),
	inspect: (file: File) => upload<InspectResult>(`${BASE}/inspect`, fileForm(file)),
	test: (input: { file?: File; text?: string }) => {
		const form = new FormData();
		if (input.file) form.append('file', input.file);
		if (input.text) form.append('text', input.text);
		return upload<TestResult>(`${BASE}/test`, form);
	}
};

function fileForm(file: File): FormData {
	const form = new FormData();
	form.append('file', file);
	return form;
}

export const chatClassificationApi = {
	setLevel: (sessionId: string, level: string, reason?: string) =>
		request<ChatClassification>(`/api/v0/chat/sessions/${encodeURIComponent(sessionId)}/classification`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(reason ? { level, reason } : { level })
		}),
	shares: (sessionId: string) => request<ChatShares>(`/api/v0/chat/sessions/${encodeURIComponent(sessionId)}/shares`),
	saveShares: (sessionId: string, body: SharesBody) =>
		request<ChatShares>(`/api/v0/chat/sessions/${encodeURIComponent(sessionId)}/shares`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(body)
		})
};

export interface SharesBody {
	link: boolean;
	subjects: { kind: 'user' | 'group'; id: string }[];
}

/** A server refusal in words: its own sentence, else the error's message. */
export function failure(err: unknown): string {
	if (err instanceof ApiError) return err.serverMessage ?? (err.detail || err.message);
	return err instanceof Error ? err.message : String(err);
}

// ---------------------------------------------------------------------------
// The draft

export interface Draft {
	levels: Level[];
	classes: DataClass[];
	markings: Marking[];
	destinations: Destination[];
	policy: Policy;
}

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

export function draftFrom(state: ClassificationState): Draft {
	return clone({ levels: sortedLevels(state.levels), classes: state.classes, markings: state.markings, destinations: state.destinations, policy: state.policy });
}

export function sortedLevels(levels: readonly Level[]): Level[] {
	return [...levels].sort((a, b) => a.rank - b.rank);
}

/** Ranks 0..n-1 in the order given. */
export function renumber(levels: readonly Level[]): Level[] {
	return levels.map((level, rank) => ({ ...level, rank }));
}

/** Move the level at `index` one step; the ranks stay contiguous. */
export function moveLevel(levels: readonly Level[], index: number, delta: -1 | 1): Level[] {
	const target = index + delta;
	if (index < 0 || index >= levels.length || target < 0 || target >= levels.length) return renumber(levels);
	const next = [...levels];
	[next[index], next[target]] = [next[target]!, next[index]!];
	return renumber(next);
}

/** A slug id from a name, made unique among `taken`. */
export function newLevelId(name: string, taken: readonly string[]): string {
	const base =
		name
			.toLowerCase()
			.normalize('NFKD')
			.replace(/[^a-z0-9]+/g, '_')
			.replace(/^_+|_+$/g, '') || 'level';
	if (!taken.includes(base)) return base;
	let n = 2;
	while (taken.includes(`${base}_${n}`)) n++;
	return `${base}_${n}`;
}

export function blankLevel(draft: Draft, name = ''): Level {
	return {
		id: newLevelId(name, draft.levels.map((l) => l.id)),
		rank: draft.levels.length,
		name,
		description: '',
		color: 'neutral',
		marking_text: '',
		mip_label_id: '',
		filename_tag: ''
	};
}

export function addLevel(draft: Draft, name = ''): Draft {
	const levels = renumber([...draft.levels, blankLevel(draft, name)]);
	return { ...draft, levels };
}

/** Where a reference to a removed level goes: the next higher level, else the highest left. */
function successor(levels: readonly Level[], removed: Level): string {
	const rest = sortedLevels(levels.filter((l) => l.id !== removed.id));
	return (rest.find((l) => l.rank > removed.rank) ?? rest[rest.length - 1])!.id;
}

/**
 * Remove a level. Classes, markings and policy references move to the next
 * higher level (the server does the same for conversations); a destination
 * that was rated at it becomes unrated. The last level cannot be removed.
 */
export function removeLevel(draft: Draft, id: string): Draft {
	const removed = draft.levels.find((l) => l.id === id);
	if (!removed || draft.levels.length <= 1) return draft;
	const sorted = sortedLevels(draft.levels);
	const to = successor(sorted, removed);
	const move = (value: string) => (value === id ? to : value);
	const p = draft.policy;
	return {
		levels: renumber(sorted.filter((l) => l.id !== id)),
		classes: draft.classes.map((c) => ({ ...c, level_id: move(c.level_id) })),
		markings: draft.markings.map((m) => ({ ...m, level_id: move(m.level_id) })),
		destinations: draft.destinations.map((d) => (d.level_id === id ? { ...d, level_id: null } : d)),
		policy: {
			...p,
			default_level: move(p.default_level),
			link_share_max_level: move(p.link_share_max_level),
			unknown_label_level: move(p.unknown_label_level),
			unreadable_level: move(p.unreadable_level)
		}
	};
}

/** Renaming a level's id is never offered; this keeps references valid after a template replaced the levels. */
export function repairReferences(draft: Draft): Draft {
	const ids = new Set(draft.levels.map((l) => l.id));
	const sorted = sortedLevels(draft.levels);
	const lowest = sorted[0]!.id;
	const highest = sorted[sorted.length - 1]!.id;
	const keep = (value: string, fallback: string) => (ids.has(value) ? value : fallback);
	const p = draft.policy;
	return {
		...draft,
		classes: draft.classes.map((c) => ({ ...c, level_id: keep(c.level_id, highest) })),
		markings: draft.markings.map((m) => ({ ...m, level_id: keep(m.level_id, highest) })),
		destinations: draft.destinations.map((d) => (d.level_id !== null && !ids.has(d.level_id) ? { ...d, level_id: null } : d)),
		policy: {
			...p,
			default_level: keep(p.default_level, lowest),
			link_share_max_level: keep(p.link_share_max_level, lowest),
			unknown_label_level: keep(p.unknown_label_level, highest),
			unreadable_level: keep(p.unreadable_level, highest)
		}
	};
}

/**
 * Start the draft from a template: its levels, classes, markings and the
 * policy fields it sets. A class whose id the current scheme already has keeps
 * its uploaded word list; destination ratings stay where the level still
 * exists. Everything else of the policy and the destinations is kept.
 */
export function applyTemplate(current: Draft, template: Template): Draft {
	const terms = new Map(current.classes.map((c) => [c.id, c.terms] as const));
	const none: Terms = { count: 0, uploaded_by: null, uploaded_at: null };
	return repairReferences({
		levels: renumber(sortedLevels(clone(template.levels))),
		classes: clone(template.classes).map((c) => ({ ...c, terms: terms.get(c.id) ?? none })),
		markings: clone(template.markings),
		destinations: current.destinations,
		policy: { ...current.policy, ...clone(template.policy), output: { ...current.policy.output, ...(template.policy.output ?? {}) } }
	});
}

export function blankClass(draft: Draft): DataClass {
	const names = draft.classes.map((c) => c.id);
	return {
		id: newLevelId('class', names),
		name: '',
		description: '',
		level_id: sortedLevels(draft.levels)[0]!.id,
		patterns: [],
		rule_question: '',
		rule_positive: '',
		rule_negative: '',
		rule_threshold: 0.5,
		action: 'raise',
		terms: { count: 0, uploaded_by: null, uploaded_at: null }
	};
}

export function togglePattern(patterns: readonly PatternId[], id: PatternId): PatternId[] {
	return patterns.includes(id) ? patterns.filter((p) => p !== id) : PATTERN_IDS.filter((p) => p === id || patterns.includes(p));
}

// ---------------------------------------------------------------------------
// Destinations

export function isUnrated(d: Pick<Destination, 'level_id'>): boolean {
	return d.level_id === null;
}

export function unratedCount(destinations: readonly Destination[]): number {
	return destinations.filter(isUnrated).length;
}

export interface DestinationGroup {
	kind: DestinationKind;
	items: Destination[];
	rated: number;
}

/** One group per kind that has any destination, unrated rows first within it. */
export function groupDestinations(destinations: readonly Destination[]): DestinationGroup[] {
	return DESTINATION_KINDS.flatMap((kind) => {
		const items = destinations.filter((d) => d.kind === kind);
		if (items.length === 0) return [];
		const unrated = items.filter(isUnrated);
		const rated = items.filter((d) => !isUnrated(d));
		return [{ kind, items: [...unrated, ...rated], rated: rated.length }];
	});
}

/** Whether the hide/gate choice applies to this destination. */
export function choosesOver(d: Pick<Destination, 'kind' | 'egress'>): boolean {
	return d.egress && (d.kind === 'tool' || d.kind === 'mcp' || d.kind === 'connector');
}

export function setRating(destinations: readonly Destination[], kind: DestinationKind, id: string, level: string | null, over?: Over): Destination[] {
	return destinations.map((d) => (d.kind === kind && d.id === id ? { ...d, level_id: level, over: over ?? d.over } : d));
}

export interface PoolAnswers {
	runs: 'own' | 'eu' | 'other';
	dpa: boolean;
	transferBasis: boolean;
	nda: boolean;
}

/**
 * The level the four guidance questions point at, or null before the
 * operator has answered anything. A pool on their own hardware is the top
 * level; a provider's pool earns up to the level below the top by agreement
 * (data processing, a transfer basis outside the EU, an NDA). It is a
 * suggestion: the rating is the operator's declaration.
 */
export function suggestPoolLevel(levels: readonly Level[], answers: PoolAnswers | null): string | null {
	if (!answers || levels.length === 0) return null;
	const sorted = sortedLevels(levels);
	if (answers.runs === 'own') return sorted[sorted.length - 1]!.id;
	const transfer = answers.runs === 'eu' || answers.transferBasis;
	const points = [answers.dpa, transfer, answers.nda].filter(Boolean).length;
	const ceiling = Math.max(0, sorted.length - 2);
	return sorted[Math.min(ceiling, Math.floor((points * (sorted.length - 1)) / 3))]!.id;
}

// ---------------------------------------------------------------------------
// Markings

/** The level whose name equals the marking's name (or value), case-insensitively. */
export function suggestMarkingLevel(levels: readonly Level[], found: { name?: string; value: string }): string {
	for (const text of [found.name, found.value]) {
		const wanted = text?.trim().toLowerCase();
		if (!wanted) continue;
		const hit = levels.find((l) => l.name.trim().toLowerCase() === wanted);
		if (hit) return hit.id;
	}
	return '';
}

/** Add the inspected markings the operator mapped to a level; ones already present are kept once. */
export function mergeMarkings(existing: readonly Marking[], found: readonly Marking[]): Marking[] {
	const out = [...existing];
	for (const m of found) {
		if (!m.level_id) continue;
		const i = out.findIndex((e) => e.kind === m.kind && e.value === m.value);
		if (i >= 0) out[i] = m;
		else out.push(m);
	}
	return out;
}

/** The first line of a CSV, split as the column picker offers it. */
export function csvColumns(text: string): string[] {
	const line = text.split(/\r?\n/, 1)[0] ?? '';
	const cells: string[] = [];
	let cell = '';
	let quoted = false;
	for (const ch of line) {
		if (ch === '"') quoted = !quoted;
		else if (ch === ',' && !quoted) {
			cells.push(cell.trim());
			cell = '';
		} else cell += ch;
	}
	cells.push(cell.trim());
	return cells.filter((c) => c.length > 0);
}

// ---------------------------------------------------------------------------
// Policy

/** A System One model is needed once any class has a rule or any destination is gated. */
export function needsClearedModel(draft: Draft): boolean {
	return draft.classes.some((c) => c.rule_question.trim() !== '') || draft.destinations.some((d) => d.level_id !== null && d.over === 'gate' && choosesOver(d));
}

// ---------------------------------------------------------------------------
// The assistant's model step

const highestLevel = (draft: Draft): Level => sortedLevels(draft.levels).at(-1)!;

/**
 * Rate every unrated pool from where its backends run: in the own network the
 * highest level, in the cloud the lowest, unknown stays unrated.
 */
export function prefillPools(draft: Draft): Draft {
	const sorted = sortedLevels(draft.levels);
	const level = { own_network: sorted.at(-1)!.id, cloud: sorted[0]!.id } as const;
	return {
		...draft,
		destinations: draft.destinations.map((d) => {
			if (d.kind !== 'pool' || d.level_id !== null || !d.hosting || d.hosting === 'unknown') return d;
			return { ...d, level_id: level[d.hosting] };
		})
	};
}

/** Whether every pool serving the model is rated at the draft's highest level. */
export function modelCleared(draft: Draft, model: Pick<SystemOneModel, 'pools'>): boolean {
	const top = highestLevel(draft).id;
	return model.pools.length > 0 && model.pools.every((pool) => draft.destinations.some((d) => d.kind === 'pool' && d.id === pool && d.level_id === top));
}

/** The one model cleared for the highest level in the draft; none when there are none or several to choose from. */
export function pickSystemOneModel(draft: Draft, models: readonly SystemOneModel[]): string {
	const cleared = models.filter((m) => modelCleared(draft, m));
	return cleared.length === 1 ? cleared[0]!.id : '';
}

/** Without a model the questions cannot be asked: patterns, markings and word lists stay. */
export function withoutQuestions(draft: Draft): Draft {
	return { ...draft, classes: draft.classes.map((c) => ({ ...c, rule_question: '', rule_positive: '', rule_negative: '' })) };
}

/** The draft the assistant saves: the chosen model set, and no questions when there is none. */
export function finalizeDraft(draft: Draft, model: string): Draft {
	const withModel = { ...draft, policy: { ...draft.policy, system_one_model: model } };
	return model === '' ? withoutQuestions(withModel) : withModel;
}

/** Activating is refused while the chosen model is not cleared for the highest level, or one is needed and none is chosen. */
export function modelBlocksActivation(draft: Draft, models: readonly SystemOneModel[], model: string): boolean {
	if (model === '') return needsClearedModel(finalizeDraft(draft, ''));
	const chosen = models.find((m) => m.id === model);
	return !chosen || !modelCleared(draft, chosen);
}

/** The destinations that send data out of AIplane without being a pool: how many, and the first few names. */
export function egressSummary(destinations: readonly Destination[], shown = 3): { count: number; names: string[] } {
	const egress = destinations.filter((d) => d.kind !== 'pool' && d.egress);
	return { count: egress.length, names: egress.slice(0, shown).map((d) => d.name) };
}

// ---------------------------------------------------------------------------
// Saving

export function buildBody(draft: Draft): ClassificationBody {
	return {
		levels: renumber(sortedLevels(draft.levels)),
		classes: draft.classes.map(({ terms: _terms, ...rest }) => rest),
		markings: draft.markings,
		assignments: draft.destinations.flatMap((d) => (d.level_id === null ? [] : [{ kind: d.kind, id: d.id, level_id: d.level_id, over: choosesOver(d) ? d.over : 'hide' }])),
		policy: draft.policy
	};
}

export interface Diff {
	levelsAdded: string[];
	levelsRemoved: string[];
	levelsRenamed: number;
	levelsReordered: boolean;
	classesAdded: string[];
	classesRemoved: string[];
	classesChanged: string[];
	markingsBefore: number;
	markingsAfter: number;
	destinationsRated: number;
	destinationsChanged: number;
	policyChanged: (keyof Policy)[];
	unrated: number;
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** What activating `draft` changes against the saved scheme `before`. */
export function diffSummary(before: ClassificationState, draft: Draft): Diff {
	const oldLevels = new Map(before.levels.map((l) => [l.id, l] as const));
	const newLevels = new Map(draft.levels.map((l) => [l.id, l] as const));
	const oldClasses = new Map(before.classes.map((c) => [c.id, c] as const));
	const newClasses = new Map(draft.classes.map((c) => [c.id, c] as const));
	const oldDest = new Map(before.destinations.map((d) => [`${d.kind}/${d.id}`, d] as const));
	const rank = (levels: Level[]) => sortedLevels(levels).map((l) => l.id);
	const strip = ({ terms: _terms, ...rest }: DataClass) => rest;
	let destinationsRated = 0;
	let destinationsChanged = 0;
	for (const d of draft.destinations) {
		const old = oldDest.get(`${d.kind}/${d.id}`);
		if (!old) continue;
		if (old.level_id === null && d.level_id !== null) destinationsRated++;
		else if (old.level_id !== d.level_id || (choosesOver(d) && old.over !== d.over)) destinationsChanged++;
	}
	return {
		levelsAdded: draft.levels.filter((l) => !oldLevels.has(l.id)).map((l) => l.name),
		levelsRemoved: before.levels.filter((l) => !newLevels.has(l.id)).map((l) => l.name),
		levelsRenamed: draft.levels.filter((l) => oldLevels.has(l.id) && !same(stripRank(oldLevels.get(l.id)!), stripRank(l))).length,
		levelsReordered: !same(rank(before.levels.filter((l) => newLevels.has(l.id))), rank(draft.levels.filter((l) => oldLevels.has(l.id)))),
		classesAdded: draft.classes.filter((c) => !oldClasses.has(c.id)).map((c) => c.name || c.id),
		classesRemoved: before.classes.filter((c) => !newClasses.has(c.id)).map((c) => c.name || c.id),
		classesChanged: draft.classes.filter((c) => oldClasses.has(c.id) && !same(strip(oldClasses.get(c.id)!), strip(c))).map((c) => c.name || c.id),
		markingsBefore: before.markings.length,
		markingsAfter: draft.markings.length,
		destinationsRated,
		destinationsChanged,
		policyChanged: (Object.keys(draft.policy) as (keyof Policy)[]).filter((k) => !same(before.policy[k], draft.policy[k])),
		unrated: unratedCount(draft.destinations)
	};
}

const stripRank = ({ rank: _rank, ...rest }: Level) => rest;

// ---------------------------------------------------------------------------
// Chat

export interface LevelOption {
	level: ChatLevel;
	/** Lowering a level asks for a reason. */
	needsReason: boolean;
}

/** The levels a conversation may move to: higher ones always, lower ones only when lowering is allowed with a reason. */
export function levelOptions(c: Pick<ChatClassification, 'level' | 'levels' | 'lowering'>): LevelOption[] {
	const current = c.level;
	if (!current) return [];
	return [...c.levels]
		.sort((a, b) => a.rank - b.rank)
		.filter((l) => l.rank > current.rank || (l.rank < current.rank && c.lowering === 'with_reason'))
		.map((level) => ({ level, needsReason: level.rank < current.rank }));
}

const BADGE: Record<LevelColor, string> = {
	neutral: 'badge-neutral',
	info: 'badge-info',
	success: 'badge-success',
	warning: 'badge-warning',
	error: 'badge-error'
};

export function levelBadgeClass(color: LevelColor): string {
	return `badge ${BADGE[color] ?? BADGE.neutral}`;
}

/** The share dialog offers people and groups when the level forbids a link. */
export function sharesBody(link: boolean, subjects: ChatShares['subjects']): SharesBody {
	return { link, subjects: subjects.map(({ kind, id }) => ({ kind, id })) };
}

/** The catalog key and arguments naming why a conversation is at its level; `null` when the server named none. */
export function causeMessage(cause: string | null | undefined): { key: string; args: Record<string, string> } | null {
	if (!cause) return null;
	for (const prefix of ['system_one', 'pattern', 'terms']) {
		if (cause.startsWith(`${prefix}:`)) return { key: `cls-cause-${prefix.replace('_', '-')}`, args: { class: cause.slice(prefix.length + 1) } };
	}
	if (cause.startsWith('marking:')) return { key: 'cls-cause-marking', args: {} };
	return { key: `cls-cause-${cause.replace(/_/g, '-')}`, args: {} };
}

/** Problems the form can see before the server does; the server still validates everything. Each is a catalog key. */
export function draftIssues(draft: Draft): string[] {
	const issues: string[] = [];
	if (draft.levels.some((l) => l.name.trim() === '')) issues.push('cls-issue-level-name');
	if (draft.classes.some((c) => c.name.trim() === '')) issues.push('cls-issue-class-name');
	return issues;
}

export function diffIsEmpty(d: Diff): boolean {
	return (
		d.levelsAdded.length + d.levelsRemoved.length + d.levelsRenamed + d.classesAdded.length + d.classesRemoved.length + d.classesChanged.length + d.destinationsRated + d.destinationsChanged + d.policyChanged.length === 0 &&
		!d.levelsReordered &&
		d.markingsBefore === d.markingsAfter
	);
}
