import test from 'node:test';
import assert from 'node:assert/strict';
import {
	addLevel,
	applyTemplate,
	buildBody,
	causeMessage,
	csvColumns,
	diffSummary,
	detectorsOf,
	draftFrom,
	egressSummary,
	finalizeDraft,
	modelBlocksActivation,
	modelCleared,
	pickDetectorModel,
	prefillPools,
	groupDestinations,
	levelOptions,
	mergeMarkings,
	moveLevel,
	needsClearedModel,
	newLevelId,
	removeLevel,
	sharesBody,
	suggestMarkingLevel,
	suggestPoolLevel,
	togglePattern,
	unratedCount,
	type ClassificationState,
	type Destination,
	type Level,
	type PatternId,
	type Policy,
	type Template
} from './classification.ts';

const level = (id: string, rank: number, name = id): Level => ({ id, rank, name, description: '', color: 'neutral', marking_text: '', mip_label_id: '', filename_tag: '' });
const policy: Policy = {
	default_level: 'public',
	lowering: 'never',
	gate_on_match: 'ask',
	user_override: true,
	link_share_max_level: 'internal',
	unknown_label_level: 'secret',
	unreadable_level: 'secret',
	detector_model: '',
	mode: 'monitor',
	mip_tenant_id: '',
	output: { visible: false, metadata: false, filename: false }
};
const dest = (kind: Destination['kind'], id: string, level_id: string | null, extra: Partial<Destination> = {}): Destination => ({ kind, id, name: id, description: '', level_id, over: 'hide', egress: false, ...extra });

const state = (): ClassificationState => ({
	active: true,
	levels: [level('public', 0, 'Public'), level('internal', 1, 'Internal'), level('secret', 2, 'Secret')],
	classes: [
		{ id: 'pii', name: 'Personal', description: '', level_id: 'internal', patterns: ['email'], rule_question: '', rule_positive: '', rule_negative: '', rule_threshold: 0.5, action: 'raise', terms: { count: 3, uploaded_by: 'a', uploaded_at: 'x' } }
	],
	markings: [{ kind: 'stamp', value: 'INTERNAL', level_id: 'internal' }],
	destinations: [dest('pool', 'p1', 'internal'), dest('pool', 'p2', null), dest('tool', 't1', null, { egress: true })],
	policy,
	templates: [],
	detector_models: []
});

test('moving a level keeps ranks contiguous from zero', () => {
	const moved = moveLevel(state().levels, 0, 1);
	assert.deepEqual(moved.map((l) => [l.id, l.rank]), [['internal', 0], ['public', 1], ['secret', 2]]);
	assert.deepEqual(moveLevel(moved, 0, -1).map((l) => l.id), ['internal', 'public', 'secret']);
	assert.deepEqual(moveLevel(state().levels, 2, 1).map((l) => l.rank), [0, 1, 2]);
});

test('a level id is a unique slug of its name', () => {
	assert.equal(newLevelId('Streng Vertraulich', []), 'streng_vertraulich');
	assert.equal(newLevelId('Internal', ['internal', 'internal_2']), 'internal_3');
	assert.equal(newLevelId('!!!', []), 'level');
	assert.equal(addLevel(draftFrom(state()), 'Top').levels.at(-1)?.rank, 3);
});

test('removing a level moves references up and unrates its destinations', () => {
	const draft = removeLevel(draftFrom(state()), 'internal');
	assert.deepEqual(draft.levels.map((l) => [l.id, l.rank]), [['public', 0], ['secret', 1]]);
	assert.equal(draft.classes[0]!.level_id, 'secret');
	assert.equal(draft.markings[0]!.level_id, 'secret');
	assert.equal(draft.policy.link_share_max_level, 'secret');
	assert.equal(draft.destinations[0]!.level_id, null);
	const lone = { ...draftFrom(state()), levels: [level('public', 0)] };
	assert.equal(removeLevel(lone, 'public').levels.length, 1);
});

test('a template replaces the scheme but keeps word lists and destination ratings that still fit', () => {
	const template: Template = {
		id: 'tlp',
		name: 'TLP',
		levels: [level('clear', 0), level('internal', 1)],
		classes: [{ id: 'pii', name: 'PII', description: '', level_id: 'internal', patterns: [], rule_question: '', rule_positive: '', rule_negative: '', rule_threshold: 0.5, action: 'confirm' }],
		markings: [{ kind: 'mip', value: 'x', level_id: 'clear' }],
		policy: { mode: 'enforce', output: { visible: true, metadata: false, filename: false } }
	};
	const before = state();
	before.policy.user_override = false;
	const draft = applyTemplate(draftFrom(before), template);
	assert.equal(draft.policy.user_override, false);
	assert.deepEqual(draft.levels.map((l) => l.id), ['clear', 'internal']);
	assert.equal(draft.classes[0]!.terms.count, 3);
	assert.equal(draft.classes[0]!.action, 'confirm');
	assert.equal(draft.policy.mode, 'enforce');
	assert.equal(draft.policy.output.visible, true);
	assert.equal(draft.policy.lowering, 'never');
	assert.equal(draft.policy.default_level, 'clear');
	assert.equal(draft.policy.unknown_label_level, 'internal');
	assert.equal(draft.destinations[0]!.level_id, 'internal');
});

test('unrated destinations are counted and listed first within their kind', () => {
	const destinations = state().destinations;
	assert.equal(unratedCount(destinations), 2);
	const groups = groupDestinations([dest('pool', 'a', 'internal'), dest('pool', 'b', null), dest('mcp', 'm', 'internal')]);
	assert.deepEqual(groups.map((g) => [g.kind, g.rated, g.items.map((d) => d.id)]), [['pool', 1, ['b', 'a']], ['mcp', 1, ['m']]]);
});

test('a marking is suggested the level with the same name, ignoring case', () => {
	const levels = state().levels;
	assert.equal(suggestMarkingLevel(levels, { name: 'secret', value: 'guid-1' }), 'secret');
	assert.equal(suggestMarkingLevel(levels, { value: ' INTERNAL ' }), 'internal');
	assert.equal(suggestMarkingLevel(levels, { name: 'Other', value: 'guid' }), '');
});

test('inspected markings merge by kind and value, unmapped ones are dropped', () => {
	const merged = mergeMarkings(state().markings, [
		{ kind: 'stamp', value: 'INTERNAL', level_id: 'secret' },
		{ kind: 'mip', value: 'g', level_id: 'public' },
		{ kind: 'mip', value: 'h', level_id: '' }
	]);
	assert.deepEqual(merged.map((m) => [m.kind, m.value, m.level_id]), [['stamp', 'INTERNAL', 'secret'], ['mip', 'g', 'public']]);
});

test('the pool guidance only suggests; own hardware is the top level, an NDA-less provider is lower', () => {
	const levels = state().levels;
	assert.equal(suggestPoolLevel(levels, null), null);
	assert.equal(suggestPoolLevel(levels, { runs: 'own', dpa: false, transferBasis: false, nda: false }), 'secret');
	assert.equal(suggestPoolLevel(levels, { runs: 'eu', dpa: true, transferBasis: false, nda: true }), 'internal');
	assert.equal(suggestPoolLevel(levels, { runs: 'other', dpa: false, transferBasis: false, nda: false }), 'public');
});

test('lowering options: higher always, lower only with a reason', () => {
	const levels = [{ id: 'public', rank: 0, name: 'Public', color: 'neutral' as const }, { id: 'internal', rank: 1, name: 'Internal', color: 'info' as const }, { id: 'secret', rank: 2, name: 'Secret', color: 'error' as const }];
	const current = levels[1]!;
	assert.deepEqual(levelOptions({ level: current, levels, lowering: 'never' }).map((o) => o.level.id), ['secret']);
	const withReason = levelOptions({ level: current, levels, lowering: 'with_reason' });
	assert.deepEqual(withReason.map((o) => [o.level.id, o.needsReason]), [['public', true], ['secret', false]]);
	assert.deepEqual(levelOptions({ level: null, levels: [], lowering: 'never' }), []);
});

test('the request body omits terms and unrated destinations, and drops over where egress does not apply', () => {
	const draft = draftFrom(state());
	draft.destinations = [dest('pool', 'p1', 'internal', { over: 'gate' }), dest('tool', 't', 'secret', { egress: true, over: 'gate' }), dest('pool', 'x', null)];
	const body = buildBody(draft);
	assert.equal('terms' in body.classes[0]!, false);
	assert.deepEqual(body.assignments, [
		{ kind: 'pool', id: 'p1', level_id: 'internal', over: 'hide' },
		{ kind: 'tool', id: 't', level_id: 'secret', over: 'gate' }
	]);
	assert.deepEqual(body.levels.map((l) => l.rank), [0, 1, 2]);
});

test('a System One model is required once a destination is gated, not for a class question', () => {
	const draft = draftFrom(state());
	assert.equal(needsClearedModel(draft), false);
	draft.classes[0]!.rule_question = 'Is it personal?';
	assert.equal(needsClearedModel(draft), false);
	draft.destinations = [dest('tool', 't', 'secret', { egress: true, over: 'gate' })];
	assert.equal(needsClearedModel(draft), true);
});

test('the diff names what activation changes', () => {
	const before = state();
	const draft = draftFrom(before);
	draft.levels = moveLevel(addLevel(draft, 'Top').levels, 0, 1);
	draft.classes[0]!.action = 'block';
	draft.policy.mode = 'enforce';
	draft.destinations[1]!.level_id = 'public';
	const diff = diffSummary(before, draft);
	assert.deepEqual(diff.levelsAdded, ['Top']);
	assert.equal(diff.levelsReordered, true);
	assert.deepEqual(diff.classesChanged, ['Personal']);
	assert.deepEqual(diff.policyChanged, ['mode']);
	assert.equal(diff.destinationsRated, 1);
	assert.equal(diff.unrated, 1);
	assert.deepEqual(diffSummary(before, draftFrom(before)).policyChanged, []);
});

test('pattern toggling keeps the catalog order', () => {
	assert.deepEqual(togglePattern(['iban'], 'email'), ['email', 'iban']);
	assert.deepEqual(togglePattern(['email', 'iban'], 'email'), ['iban']);
});

test('the csv column picker reads the header line, quotes included', () => {
	assert.deepEqual(csvColumns('id,"last, first",term\r\n1,2,3'), ['id', 'last, first', 'term']);
	assert.deepEqual(csvColumns(''), []);
});

test('share bodies carry kind and id only', () => {
	assert.deepEqual(sharesBody(false, [{ kind: 'user', id: 'u1', name: 'Ada' }]), { link: false, subjects: [{ kind: 'user', id: 'u1' }] });
});

test('a level cause names its catalog message, a detector cause carries the class', () => {
	assert.equal(causeMessage(null), null);
	assert.deepEqual(causeMessage('scheme_change'), { key: 'cls-cause-scheme-change', args: {} });
	assert.deepEqual(causeMessage('system_one:pii'), { key: 'cls-cause-system-one', args: { class: 'pii' } });
});

test('pattern, word-list, marking and retrieval causes have catalog messages', () => {
	assert.deepEqual(causeMessage('pattern:iban'), { key: 'cls-cause-pattern', args: { class: 'iban' } });
	assert.deepEqual(causeMessage('terms:trade_secrets'), { key: 'cls-cause-terms', args: { class: 'trade_secrets' } });
	assert.deepEqual(causeMessage('marking:mip:9fbde396-1a24-4c79-8edf-9254a0f35055'), { key: 'cls-cause-marking', args: {} });
	assert.deepEqual(causeMessage('retrieval'), { key: 'cls-cause-retrieval', args: {} });
});

const modelStep = (): ClassificationState => {
	const s = state();
	s.destinations = [
		dest('pool', 'qwen', null, { hosting: 'own_network', hosts: ['llm01.int'] }),
		dest('pool', 'zai', null, { hosting: 'cloud', hosts: ['api.z.ai'] }),
		dest('pool', 'odd', null, { hosting: 'unknown', hosts: ['gone.example'] }),
		dest('tool', 'search_web', null, { egress: true }),
		dest('tool', 'clock', null),
		dest('mcp', 'github', null, { egress: true })
	];
	s.classes[0]!.rule_question = 'Is it personal?';
	s.classes[0]!.rule_positive = 'a name';
	s.classes[0]!.rule_negative = 'weather';
	return s;
};
const models = [
	{ id: 'qwen-big', kind: 'system_one' as const, level_id: null, cleared_for_highest: false, pools: ['qwen'] },
	{ id: 'glm', kind: 'system_one' as const, level_id: null, cleared_for_highest: false, pools: ['zai'] }
];
const rate = (draft: ReturnType<typeof draftFrom>, id: string, level: string | null) => ({ ...draft, destinations: draft.destinations.map((d) => (d.id === id ? { ...d, level_id: level } : d)) });

test('pools are prefilled from where their backends run, unknown stays unrated', () => {
	const draft = prefillPools(draftFrom(modelStep()));
	const level = (id: string) => draft.destinations.find((d) => d.id === id)!.level_id;
	assert.equal(level('qwen'), 'secret');
	assert.equal(level('zai'), 'public');
	assert.equal(level('odd'), null);
	assert.equal(level('search_web'), null);
});

test('a pool the operator already rated keeps its rating', () => {
	const draft = prefillPools(rate(draftFrom(modelStep()), 'qwen', 'internal'));
	assert.equal(draft.destinations.find((d) => d.id === 'qwen')!.level_id, 'internal');
});

test('the System One model is preselected only when exactly one is cleared in the draft', () => {
	const draft = prefillPools(draftFrom(modelStep()));
	assert.equal(pickDetectorModel(draft, models), 'qwen-big');
	assert.equal(pickDetectorModel(rate(draft, 'zai', 'secret'), models), '');
	assert.equal(pickDetectorModel(rate(draft, 'qwen', null), models), '');
	assert.equal(pickDetectorModel(draft, []), '');
});

const chat = (id: string, pools: string[]) => ({ id, kind: 'chat' as const, level_id: null, cleared_for_highest: false, pools });

test('a cleared chat model is preselected only when no System One model exists', () => {
	const draft = prefillPools(draftFrom(modelStep()));
	assert.equal(pickDetectorModel(draft, [...models, chat('chat-a', ['qwen'])]), 'qwen-big');
	assert.equal(pickDetectorModel(rate(draft, 'zai', 'secret'), [...models, chat('chat-a', ['qwen'])]), '');
	assert.equal(pickDetectorModel(draft, [chat('chat-a', ['qwen'])]), 'chat-a');
	assert.equal(pickDetectorModel(draft, [chat('zeta', ['qwen']), chat('alpha', ['qwen']), chat('glm-chat', ['zai'])]), 'alpha');
	assert.equal(pickDetectorModel(draft, [chat('glm-chat', ['zai'])]), '');
});

test('a model is cleared only when all its pools are at the highest level', () => {
	const draft = prefillPools(draftFrom(modelStep()));
	assert.equal(modelCleared(draft, { pools: ['qwen'] }), true);
	assert.equal(modelCleared(draft, { pools: ['qwen', 'zai'] }), false);
	assert.equal(modelCleared(draft, { pools: [] }), false);
});

test('finalizing sets the model and keeps the questions with or without one', () => {
	const draft = prefillPools(draftFrom(modelStep()));
	assert.equal(finalizeDraft(draft, 'qwen-big').policy.detector_model, 'qwen-big');
	assert.equal(finalizeDraft(draft, 'qwen-big').classes[0]!.rule_question, 'Is it personal?');
	assert.equal(finalizeDraft(draft, '').policy.detector_model, '');
	assert.equal(finalizeDraft(draft, '').classes[0]!.rule_question, 'Is it personal?');
});

test('activation is blocked for a model whose pool is not at the highest level', () => {
	const draft = prefillPools(draftFrom(modelStep()));
	assert.equal(modelBlocksActivation(draft, models, 'qwen-big'), false);
	assert.equal(modelBlocksActivation(draft, models, 'glm'), true);
	assert.equal(modelBlocksActivation(draft, models, ''), false);
});

test('egress destinations outside pools are counted with their first names', () => {
	assert.deepEqual(egressSummary(modelStep().destinations), { count: 2, names: ['search_web', 'github'] });
	assert.deepEqual(egressSummary(modelStep().destinations, 1), { count: 2, names: ['search_web'] });
});

test('a class lists its patterns, its rule question and its word list as detectors', () => {
	const cls = { ...draftFrom(modelStep()).classes[0]!, patterns: ['iban', 'email'] as PatternId[], rule_question: 'Is it personal?', terms: { count: 3, uploaded_by: null, uploaded_at: null } };
	assert.deepEqual(detectorsOf(cls, true), [
		{ kind: 'pattern', id: 'email' },
		{ kind: 'pattern', id: 'iban' },
		{ kind: 'rule', active: true },
		{ kind: 'terms', count: 3 }
	]);
	assert.deepEqual(detectorsOf(cls, false)[2], { kind: 'rule', active: false });
	assert.deepEqual(detectorsOf({ ...cls, patterns: [], rule_question: '  ', terms: { count: 0, uploaded_by: null, uploaded_at: null } }, true), []);
});
