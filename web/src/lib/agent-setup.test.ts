import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';

import { cleanSpec, ensureShape, type AgentResources, type Grant, type GrantableItem, type GrantableKind, type Spec } from './agents.ts';
import {
	SLOT_KINDS,
	TEMPLATES,
	TONE_LINES,
	abilities,
	applyTemplate,
	applySuggestedRules,
	requireKnowledgeSearch,
	asStep,
	checklist,
	deriveBind,
	identFrom,
	identityWriter,
	isBlank,
	publishState,
	liveUses,
	newSecret,
	originOf,
	readBasics,
	readHandoffs,
	readIdentity,
	readScope,
	readColor,
	readSite,
	readVoice,
	readSlots,
	sectionStatus,
	setAbility,
	setKnowledge,
	slotDef,
	slotKind,
	slotLabel,
	slotsForIdentity,
	stepForPath,
	voiceMissing,
	speechVoices,
	summary,
	suggestedMethod,
	suggestedRules,
	suggestedSlotRows,
	suggestedTone,
	templateSpec,
	topicsNeedingIdentity,
	unique,
	writeBasics,
	writeHandoffs,
	notifyOn,
	notifyEverywhere,
	setNotify,
	sharedNotify,
	writeIdentity,
	writeScope,
	writeColor,
	writeSite,
	writeVoice,
	writeSlots,
	type Basics,
	type Handoffs,
	type Identity,
	type SlotRow,
	defaultOutOfReach,
	modelGrantFor,
	modelPickerOptions,
	modelsInUse,
	setupErrorMessage,
	voiceOffered
} from './agent-setup.ts';

const tr = (key: string) => `«${key}»`;
const labels = { email: 'E-mail', name: 'Name', customerNumber: 'Customer number' };

/** What the advanced editor makes of a spec: shaped for its forms, then cleaned for saving. */
const sortKeys = (v: unknown): unknown =>
	Array.isArray(v) ? v.map(sortKeys) : v && typeof v === 'object' ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, sortKeys((v as Spec)[k])])) : v;
/** What the advanced editor and a save make of a spec: cleaned for saving, stored by the server (which sorts the keys), shaped again for the forms. */
const throughEditor = (spec: Spec): Spec => ensureShape(sortKeys(cleanSpec(ensureShape(spec))) as Spec);

test('names become spec identifiers, unique within what is taken', () => {
	assert.equal(identFrom('E-Mail-Adresse', 'detail'), 'e_mail_adresse');
	assert.equal(identFrom('Größe', 'detail'), 'grosse');
	assert.equal(identFrom('2nd line', 'detail'), 'detail_2nd_line');
	assert.equal(identFrom('账单', 'handoff'), 'handoff');
	assert.equal(identFrom('x'.repeat(80), 'd').length, 48);
	assert.equal(unique('topic', ['topic', 'topic_2']), 'topic_3');
	assert.equal(unique('fresh', ['topic']), 'fresh');
	assert.equal(asStep('scope'), 'scope');
	assert.equal(asStep('nope'), null);
});

test('task and tone round-trip through the response instructions', () => {
	const model: Basics = {
		name: 'Harald',
		task: 'Help croit customers.',
		tones: ['friendly', 'formal'],
		language: 'de',
		extra: 'Never use emojis.',
		model: 'chat-large'
	};
	const spec = ensureShape({});
	writeBasics(spec, model);
	assert.equal(spec.main.instructions.response, `${TONE_LINES.friendly}\n${TONE_LINES.formal}\nAlways answer in German.\nNever use emojis.`);
	assert.equal(spec.profile.display, 'Harald');
	assert.equal(spec.main.model, 'chat-large');
	assert.deepEqual(readBasics(throughEditor(spec)), model);
	writeBasics(spec, { ...model, model: '' });
	assert.equal(spec.main.model, undefined, 'the default is no key at all');
});

test('free-written response instructions stay as they are', () => {
	const spec = { main: { instructions: { response: 'Friendly, short, in the visitor’s language.' } } };
	const b = readBasics(spec);
	assert.deepEqual(b.tones, []);
	assert.equal(b.language, null);
	assert.equal(b.extra, 'Friendly, short, in the visitor’s language.');
	writeBasics(spec, b);
	assert.equal((spec as Spec).main.instructions.response, 'Friendly, short, in the visitor’s language.');
});

const labels3 = { empty: 'Default (qwen)', gdpr: 'GDPR', nda: 'NDA' };

test('the model picker offers the default, what the manager may grant, and what the agent already uses', () => {
	const r: AgentResources = {
		...resources,
		models: { chat: [{ id: 'qwen', gdpr: true, nda: false }, { id: 'auto', gdpr: false, nda: false }], transcription: [{ id: 'whisper', gdpr: true, nda: true }], speech: [] },
		defaults: { chat: 'qwen', transcription: 'whisper', speech: null }
	};
	const options = modelPickerOptions('chat', 'legacy', ['admin-only', 'whisper'], r, labels3);
	assert.deepEqual(
		options.map((o) => [o.value, o.label, (o.badges ?? []).map((b) => `${b.label}:${b.tone}`).join(',')]),
		[
			['', 'Default (qwen)', ''],
			['qwen', 'qwen', 'GDPR:success,NDA:error'],
			['auto', 'auto', 'GDPR:error,NDA:error'],
			['admin-only', 'admin-only', ''],
			['legacy', 'legacy', '']
		],
		'a held model listed under another kind stays out; one listed nowhere and the named one are shown plainly'
	);
	assert.deepEqual(
		modelPickerOptions('speech', '', [], r, { ...labels3, empty: null }).map((o) => o.value),
		[],
		'no default, no entry for it'
	);
});

test('a model choice stages the grant of what the agent will run on', () => {
	const r: AgentResources = {
		...resources,
		models: { chat: [{ id: 'qwen', gdpr: true, nda: true }], transcription: [], speech: [] },
		defaults: { chat: 'qwen', transcription: 'whisper', speech: null }
	};
	assert.equal(modelGrantFor('chat', 'big', r), 'big');
	assert.equal(modelGrantFor('chat', '', r), 'qwen', 'the default, when the manager may grant it');
	assert.equal(modelGrantFor('transcription', '', r), null, 'a default the manager does not hold is never staged');
	assert.equal(modelGrantFor('speech', '', r), null);
	assert.equal(defaultOutOfReach('transcription', '', [], r), 'whisper', 'the step says why the default cannot be given');
	assert.equal(defaultOutOfReach('transcription', '', ['whisper'], r), null, 'the agent holds it already');
	assert.equal(defaultOutOfReach('transcription', 'other', [], r), null);
	assert.equal(defaultOutOfReach('chat', '', [], r), null);
});

test('the models a spec runs on include the defaults its unset keys fall back to', () => {
	const defaults = { chat: 'qwen', transcription: 'whisper', speech: 'tts-1' };
	const spec: Spec = {
		scope: { classifier_model: 'guard' },
		router: { kind: 'classifier', model: 'small' },
		publish: { voice: { input: true, output: false, speech_model: 'piper' } }
	};
	assert.deepEqual(modelsInUse(spec, defaults).sort(), ['guard', 'piper', 'qwen', 'small', 'whisper']);
	spec.main = { model: 'big' };
	assert.ok(!modelsInUse(spec, defaults).includes('qwen'), 'a named main model replaces the default');
	assert.deepEqual(modelsInUse({}, undefined), []);
});

test('the scope step writes #115 scope and drops an empty one', () => {
	const spec: Spec = { scope: { classifier_model: 'guard' } };
	writeScope(spec, { topics: [' Ceph ', '', 'Licences'], refusal: 'Only croit.', strict: true });
	assert.deepEqual(spec.scope, { topics: ['Ceph', 'Licences'], refusal: 'Only croit.', strict: true, classifier_model: 'guard' });
	assert.deepEqual(readScope(throughEditor(spec)), { topics: ['Ceph', 'Licences'], refusal: 'Only croit.', strict: true });

	const empty: Spec = { scope: { topics: [] } };
	writeScope(empty, { topics: [], refusal: '', strict: false });
	assert.equal(empty.scope, undefined);
});

test('every friendly slot kind reads back as itself, through the advanced editor too', () => {
	for (const kind of SLOT_KINDS) {
		const def = slotDef(kind, 'Label', kind === 'choice' ? ['a', 'b'] : []);
		assert.equal(slotKind(def), kind, kind);
		assert.equal(slotKind(throughEditor({ state: { x: def } }).state.x), kind, `${kind} after the editor`);
	}
	assert.equal(slotKind({ type: 'string', max_length: 50, set_by: ['llm'] }), 'custom');
	assert.equal(slotKind({ type: 'email', set_by: ['host'] }), 'custom');
});

test('details round-trip; a fresh row takes its key from its label, managed and custom slots survive', () => {
	const custom = { type: 'string', min_length: 3, set_by: ['llm'] };
	const spec: Spec = {
		state: {
			email: slotDef('email', 'E-mail'),
			legacy: custom,
			verified: { type: 'subject', set_by: ['host'] },
			topic: { type: 'enum', values: ['x'], set_by: ['llm'] }
		}
	};
	const rows = readSlots(spec);
	assert.deepEqual(rows.map((r) => [r.key, r.kind]), [['email', 'email'], ['legacy', 'custom']]);
	const next: SlotRow[] = [...rows, { key: '', label: 'Customer number', kind: 'customer_number', values: [], fresh: true }, { key: '', label: 'Plan', kind: 'choice', values: ['basic', 'pro'], fresh: true }];
	writeSlots(spec, next);
	assert.deepEqual(Object.keys(spec.state), ['email', 'legacy', 'customer_number', 'plan', 'verified', 'topic']);
	const back = readSlots(throughEditor(spec));
	assert.deepEqual(back, [
		{ key: 'email', label: 'E-mail', kind: 'email', values: [] },
		{ key: 'legacy', label: 'legacy', kind: 'custom', values: [] },
		{ key: 'customer_number', label: 'Customer number', kind: 'customer_number', values: [] },
		{ key: 'plan', label: 'Plan', kind: 'choice', values: ['basic', 'pro'] }
	], 'the order the person gave survives the server sorting the keys');
	assert.deepEqual(spec.state.legacy, { ...custom, order: 1 });
	writeSlots(spec, back.filter((r) => r.key !== 'legacy'));
	assert.equal(spec.state.legacy, undefined);
});

const identity = (over: Partial<Identity>): Identity => ({ method: 'none', connector: '', tool: '', issuer: '', audience: '', secret: '', secretSet: false, others: 0, ...over });

test('each identity method writes its verifier and the confirmed slot, and reads back', () => {
	const spec: Spec = {};
	writeIdentity(spec, identity({ method: 'email_code', connector: 'erp' }), labels);
	assert.deepEqual(spec.verifiers.identity, { kind: 'mcp_code', email_slot: 'email', writes: { verified: 'result' }, connector: 'erp' });
	assert.deepEqual(spec.state.verified, { type: 'subject', set_by: ['verifier:identity'] });
	assert.equal(spec.state.email.type, 'email');
	assert.deepEqual(slotsForIdentity(spec), ['email']);
	assert.equal(readIdentity(throughEditor(spec)).connector, 'erp');
	assert.equal(readIdentity(spec).method, 'email_code');

	writeIdentity(spec, identity({ method: 'customer_lookup', tool: 'mcp__erp__find' }), labels);
	assert.equal(spec.verifiers.identity.kind, 'lookup');
	assert.equal(spec.verifiers.identity.assurance, 'low');
	assert.deepEqual(slotsForIdentity(spec).sort(), ['customer_number', 'name']);
	assert.equal(slotKind(spec.state.customer_number), 'customer_number');
	assert.deepEqual(readIdentity(throughEditor(spec)).tool, 'mcp__erp__find');

	writeIdentity(spec, identity({ method: 'signed_in', issuer: 'https://shop.example', audience: 'support', secret: 'x'.repeat(40) }), labels);
	assert.equal(spec.verifiers.identity.algorithm, 'HS256');
	assert.deepEqual(spec.state.verified.set_by, ['host']);
	assert.equal(identityWriter(spec), 'host');
	const read = readIdentity(throughEditor(spec));
	assert.equal(read.issuer, 'https://shop.example');
	assert.equal(read.audience, 'support');

	writeIdentity(spec, identity({ method: 'none' }), labels);
	assert.equal(spec.verifiers, undefined);
	assert.equal(spec.state.verified, undefined);
	assert.equal(readIdentity(spec).method, 'none');
});

test('a stored secret stays sealed until a new one is typed', () => {
	const spec: Spec = { verifiers: { identity: { kind: 'host_jwt', algorithm: 'HS256', secret_sealed: 'sealed', issuer: 'i', audience: 'a' } } };
	const read = readIdentity(spec);
	assert.equal(read.secretSet, true);
	writeIdentity(spec, read, labels);
	assert.equal(spec.verifiers.identity.secret_sealed, 'sealed');
	assert.equal(spec.verifiers.identity.secret, undefined);
	writeIdentity(spec, { ...read, secret: 'n'.repeat(40) }, labels);
	assert.equal(spec.verifiers.identity.secret_sealed, undefined);
	assert.equal(newSecret().length, 48);
});

test('changing the identity method moves the hand-off gates to the new writer', () => {
	const spec: Spec = {};
	writeIdentity(spec, identity({ method: 'email_code' }), labels);
	writeHandoffs(spec, { rules: [{ route: null, topic: 'Invoices', details: false, identity: true, target: { kind: 'agent', id: 'a1' }, bind: {} }], fallback: false, custom: [] });
	assert.deepEqual(spec.routes.invoices.when.all[2], { slot: 'verified', provenance: 'verifier:identity' });
	writeIdentity(spec, identity({ method: 'signed_in' }), labels);
	assert.deepEqual(spec.routes.invoices.when.all[2], { slot: 'verified', provenance: 'host' });
	assert.deepEqual(topicsNeedingIdentity(spec), ['Invoices']);
});

test('hand-off sentences become routes with derived gates, and read back unchanged', () => {
	const spec: Spec = { routes: { legacy: { when: { slot: 'x', set: true }, agent: 'z', task: 't' } }, state: { x: { type: 'string', set_by: ['llm'] } } };
	writeIdentity(spec, identity({ method: 'email_code', connector: 'erp' }), labels);
	const model: Handoffs = {
		rules: [
			{ route: null, topic: 'Invoices', details: false, identity: true, target: { kind: 'agent', id: 'billing-id' }, bind: { customer: 'state.verified.customer' } },
			{ route: null, topic: 'Technik', details: false, identity: false, target: { kind: 'human' }, bind: {} },
			{ route: null, topic: '  ', details: false, identity: false, target: { kind: 'human' }, bind: {} }
		],
		fallback: true,
		custom: ['legacy']
	};
	writeHandoffs(spec, model);
	assert.deepEqual(Object.keys(spec.routes), ['invoices', 'technik', 'legacy', 'fallback']);
	assert.deepEqual(spec.router, { kind: 'rules', order: ['invoices', 'technik', 'legacy', 'fallback'] });
	assert.deepEqual(spec.routes.invoices, {
		description: 'Invoices',
		when: { all: [{ slot: 'topic', eq: 'Invoices' }, { slot: 'request', set: true }, { slot: 'verified', provenance: 'verifier:identity' }] },
		agent: 'billing-id',
		task: 'Request about {topic}: {request}',
		bind: { customer: 'state.verified.customer' }
	});
	assert.deepEqual(spec.routes.technik.human, {});
	assert.deepEqual(spec.state.topic.values, ['Invoices', 'Technik']);
	assert.equal(spec.state.request.type, 'string');

	const back = readHandoffs(throughEditor(spec));
	assert.deepEqual(back, {
		rules: [
			{ route: 'invoices', topic: 'Invoices', details: false, identity: true, target: { kind: 'agent', id: 'billing-id' }, bind: { customer: 'state.verified.customer' } },
			{ route: 'technik', topic: 'Technik', details: false, identity: false, target: { kind: 'human' }, bind: {} }
		],
		fallback: true,
		custom: ['legacy']
	});

	writeHandoffs(spec, { rules: [], fallback: false, custom: ['legacy'] });
	assert.deepEqual(Object.keys(spec.routes), ['legacy']);
	assert.equal(spec.state.topic, undefined);
	assert.equal(spec.state.request, undefined);
	assert.deepEqual(spec.router, { kind: 'rules', order: ['legacy'] });
	writeHandoffs(spec, { rules: [], fallback: false, custom: [] });
	assert.equal(spec.router, undefined);
});

test('a hand-off can wait until every detail is collected, and follows the details step', () => {
	const spec: Spec = {};
	writeSlots(spec, [
		{ key: 'company', label: 'Company', kind: 'text', values: [] },
		{ key: 'email', label: 'E-mail', kind: 'email', values: [] }
	]);
	writeHandoffs(spec, { rules: [{ route: null, topic: 'Qualifizierte Vertriebsanfrage', details: true, identity: false, target: { kind: 'human' }, bind: {} }], fallback: false, custom: [] });
	assert.deepEqual(spec.routes.qualifizierte_vertriebsanfrage.when.all, [
		{ slot: 'topic', eq: 'Qualifizierte Vertriebsanfrage' },
		{ slot: 'request', set: true },
		{ slot: 'company', set: true },
		{ slot: 'email', set: true }
	]);
	const back = readHandoffs(throughEditor(spec));
	assert.deepEqual(back.custom, []);
	assert.equal(back.rules[0].details, true);

	writeSlots(spec, [...readSlots(spec), { key: 'size', label: 'Storage size', kind: 'text', values: [], fresh: true }]);
	assert.deepEqual(
		spec.routes.qualifizierte_vertriebsanfrage.when.all.slice(2).map((leaf: Spec) => leaf.slot),
		['company', 'email', 'storage_size'],
		'a new detail joins the gate'
	);

	writeIdentity(spec, identity({ method: 'email_code', connector: 'erp' }), labels);
	writeHandoffs(spec, { ...readHandoffs(spec), rules: [{ ...readHandoffs(spec).rules[0], identity: true }] });
	const both = readHandoffs(throughEditor(spec)).rules[0];
	assert.equal(both.details && both.identity, true);
	assert.deepEqual(spec.routes.qualifizierte_vertriebsanfrage.when.all.at(-1), { slot: 'verified', provenance: 'verifier:identity' });
});

test('a hand-written route on one detail stays custom and is kept verbatim when the details change', () => {
	const text = (order: number) => ({ type: 'string', max_length: 200, set_by: ['llm'], order });
	const refund = {
		description: 'Refund',
		when: { all: [{ slot: 'topic', eq: 'Refund' }, { slot: 'request', set: true }, { slot: 'order_id', set: true }] },
		human: {}
	};
	const spec: Spec = {
		state: { order_id: text(0), email: text(1), phone: text(2), topic: { type: 'enum', values: ['Refund', 'Lead'], set_by: ['llm'] } },
		routes: {
			refund: structuredClone(refund),
			lead: {
				description: 'Lead',
				when: { all: [{ slot: 'topic', eq: 'Lead' }, { slot: 'request', set: true }, { slot: 'phone', set: true }, { slot: 'order_id', set: true }, { slot: 'email', set: true }] },
				human: {}
			}
		},
		router: { kind: 'rules', order: ['refund', 'lead'] }
	};
	const h = readHandoffs(spec);
	assert.deepEqual(h.custom, ['refund']);
	assert.deepEqual(h.rules.map((r) => [r.topic, r.details]), [['Lead', true]], 'every detail, in any order, is the details rule');

	writeSlots(spec, [...readSlots(spec), { key: 'fax', label: 'Fax', kind: 'text', values: [], fresh: true }]);
	assert.deepEqual(spec.routes.refund, refund);
	assert.deepEqual(
		spec.routes.lead.when.all.slice(2).map((leaf: Spec) => leaf.slot),
		['order_id', 'email', 'phone', 'fax']
	);
});

test('the identity step regates a hand-off waiting for every detail when it adds the slots it reads', () => {
	const spec: Spec = {};
	writeSlots(spec, [{ key: 'company', label: 'Company', kind: 'text', values: [] }]);
	writeHandoffs(spec, { rules: [{ route: null, topic: 'Lead', details: true, identity: false, target: { kind: 'human' }, bind: {} }], fallback: false, custom: [] });
	writeIdentity(spec, identity({ method: 'email_code', connector: 'erp' }), labels);
	assert.deepEqual(spec.routes.lead.when.all.slice(2).map((leaf: Spec) => leaf.slot), ['company', 'email']);
	assert.equal(readHandoffs(spec).rules[0].details, true);
});

test('a suggested hand-off that waits for the identity sets up the proposed check, so it keeps its gate', () => {
	const spec: Spec = {};
	const proposed = [{ topic: 'Refunds', target: 'human', identity: true }];
	const applied = applySuggestedRules(spec, [], proposed, { method: 'email_code' }, labels);
	assert.equal(applied.identity, true);
	assert.equal(readIdentity(spec).method, 'email_code');
	writeHandoffs(spec, { rules: applied.rules, fallback: false, custom: [] });
	assert.deepEqual(spec.routes.refunds.when.all.at(-1), { slot: 'verified', provenance: 'verifier:identity' });
	assert.equal(readHandoffs(throughEditor(spec)).rules[0].identity, true);

	const signedIn: Spec = { verifiers: { identity: { kind: 'host_jwt' } } };
	assert.equal(applySuggestedRules(signedIn, [], proposed, { method: 'email_code' }, labels).identity, false, 'an existing check stays');
	assert.equal(applySuggestedRules({}, [], proposed, { method: 'none' }, labels).identity, false);
	assert.equal(applySuggestedRules({}, [], [{ topic: 'X', target: 'human' }], { method: 'email_code' }, labels).identity, false);
});

test('suggested knowledge that cannot be searched fails the apply with the reason', () => {
	assert.doesNotThrow(() => requireKnowledgeSearch([], false, tr));
	assert.doesNotThrow(() => requireKnowledgeSearch(['docs'], true, tr));
	assert.throws(
		() => requireKnowledgeSearch(['docs'], false, tr),
		(err: Parameters<typeof setupErrorMessage>[0]) => setupErrorMessage(err, tr) === '«agents-setup-knowledge-no-search»'
	);
});

test('a specialist’s route values come from trusted slots or the confirmed identity', () => {
	const specialist = { main: { tool_resources: { mcp__erp__invoices: { bind: { customer_id: 'route.customer', region: { const: 'eu' } } }, other: { bind: { x: 'route.plan' } } } } };
	const spec: Spec = { state: { plan: { type: 'string', set_by: ['host'] } } };
	assert.deepEqual(deriveBind(specialist, spec), { bind: { plan: 'state.plan' }, missing: ['customer'] });
	spec.state.verified = { type: 'subject', set_by: ['host'] };
	assert.deepEqual(deriveBind(specialist, spec), { bind: { customer: 'state.verified.customer', plan: 'state.plan' }, missing: [] });
	assert.deepEqual(deriveBind(null, spec), { bind: {}, missing: [] });
});

const item = (kind: GrantableKind, key: string, title: string, refs: string[], tools: string[], description = ''): GrantableItem => ({
	key,
	kind: kind === 'connector' ? 'tool' : kind,
	title,
	description,
	group: kind,
	order: 0,
	icon: null,
	grant: { kind, refs },
	tools,
	editable: false,
	config_url: null
});
const resources: AgentResources = {
	models: { chat: [{ id: 'mid', gdpr: true, nda: true }], transcription: [], speech: [] },
	items: [
		item('rag_collection', '7', 'croit docs', ['7'], []),
		item('tool', 'search_web', 'Web search', ['search_web'], ['search_web'], 'Searches the web'),
		item('tool', 'memory', 'Memory', ['remember', 'recall'], ['remember', 'recall']),
		item('tool', 'rag_search', 'Knowledge search', ['rag_search'], ['rag_search']),
		item('tool', 'rag_list_collections', 'List collections', ['rag_list_collections'], ['rag_list_collections']),
		item('connector', 'mcp__jira', 'Jira', ['jira'], ['mcp__jira__create', 'mcp__jira__get']),
		item('skill', 'brand-voice', 'Brand voice', ['brand-voice'], [])
	]
};
const grant = (kind: Grant['kind'], ref: string): Grant => ({ kind, ref, granted_by: 'admin', granted_at: '' });

test('ability cards: what the manager may grant, and what the agent has that they may not', () => {
	const spec: Spec = { main: { tools: ['search_web', 'mcp__erp__invoices', 'netcheck'], skills: ['secret-skill'] } };
	const cards = abilities(spec, [grant('rag_collection', '7'), grant('rag_collection', '9')], resources);
	const view = cards.map((c) => `${c.kind}:${c.ref}:${c.on ? 'on' : 'off'}:${c.holdable ? 'mine' : 'locked'}`);
	assert.deepEqual(view, [
		'rag_collection:7:on:mine',
		'tool:search_web:on:mine',
		'tool:memory:off:mine',
		'connector:jira:off:mine',
		'skill:brand-voice:off:mine',
		'rag_collection:9:on:locked',
		'tool:netcheck:on:locked',
		'connector:erp:on:locked',
		'skill:secret-skill:on:locked'
	]);
	const title = (ref: string) => cards.find((c) => c.ref === ref)?.item;
	assert.equal(title('jira')?.title, 'Jira', 'the server names what the manager holds');
	assert.deepEqual([title('netcheck')?.title, title('netcheck')?.description, title('netcheck')?.editable], ['netcheck', '', false], 'what they do not hold: its reference alone');
});

test('a catalog entry standing for several tools switches all of them', () => {
	const spec: Spec = ensureShape({});
	const memory = abilities(spec, [], resources).find((c) => c.ref === 'memory')!;
	assert.deepEqual(memory.refs, ['remember', 'recall']);
	setAbility(spec, memory, true);
	assert.deepEqual(spec.main.tools, ['remember', 'recall']);
	assert.ok(abilities(spec, [], resources).find((c) => c.ref === 'memory')?.on);
	setAbility(spec, memory, false);
	assert.deepEqual(spec.main.tools, []);
});

test('switching abilities edits the spec, and knowledge binds a single collection', () => {
	const spec: Spec = ensureShape({});
	setAbility(spec, { kind: 'connector', ref: 'jira', tools: ['mcp__jira__create', 'mcp__jira__get'] }, true);
	setAbility(spec, { kind: 'skill', ref: 'brand-voice', tools: [] }, true);
	setAbility(spec, { kind: 'tool', ref: 'search_web', tools: ['search_web'] }, true);
	setKnowledge(spec, ['croit docs'], true);
	assert.deepEqual(spec.main.tools, ['mcp__jira__create', 'mcp__jira__get', 'search_web', 'rag_search']);
	assert.deepEqual(spec.main.tool_resources.rag_search, { bind: { collection: { const: 'croit docs' } } });
	const after = abilities(throughEditor(spec), [grant('rag_collection', '7')], resources).filter((c) => c.on).map((c) => c.ref);
	assert.deepEqual(after, ['7', 'search_web', 'jira', 'brand-voice']);

	setKnowledge(spec, ['croit docs', 'faq'], true);
	assert.deepEqual(spec.main.tool_resources.rag_search.bind, {});
	assert.ok(spec.main.tools.includes('rag_list_collections'));
	setKnowledge(spec, [], true);
	setAbility(spec, { kind: 'connector', ref: 'jira', tools: [] }, false);
	assert.deepEqual(spec.main.tools, ['search_web']);
	assert.equal(spec.main.tool_resources.rag_search, undefined);
});

test('a grant the published version relies on is kept', () => {
	const live = { main: { model: 'mid', tools: ['rag_search', 'mcp__jira__get'], skills: ['brand-voice'] }, verifiers: { identity: { kind: 'mcp_code', connector: 'erp' } } };
	assert.ok(liveUses(live, 'model', 'mid'));
	assert.ok(liveUses(live, 'connector', 'jira'));
	assert.ok(liveUses(live, 'connector', 'erp'));
	assert.ok(liveUses(live, 'rag_collection', '7'));
	assert.ok(liveUses(live, 'skill', 'brand-voice'));
	assert.ok(!liveUses(live, 'tool', 'search_web'));
	assert.ok(!liveUses(null, 'tool', 'search_web'));
	assert.ok(liveUses({ publish: { voice: { speech_model: 'tts' } } }, 'model', 'tts'), 'a live voice model stays granted');
	const defaults = { chat: 'qwen', transcription: null, speech: null };
	assert.ok(liveUses({ main: {} }, 'model', 'qwen', defaults), 'the default a live agent runs on stays granted');
	assert.ok(!liveUses({ main: { model: 'mid' } }, 'model', 'qwen', defaults));
});

test('the website step keeps only web origins', () => {
	assert.equal(originOf('www.croit.io/de/kontakt'), 'https://www.croit.io');
	assert.equal(originOf('http://localhost:3000/x'), 'http://localhost:3000');
	assert.equal(originOf('ftp://x'), null);
	assert.equal(originOf('not a url'), null);
	const spec: Spec = {};
	writeSite(spec, ['https://www.croit.io/x', 'www.croit.io', 'nonsense here']);
	assert.deepEqual(readSite(throughEditor(spec)), ['https://www.croit.io']);
	writeSite(spec, []);
	assert.equal(spec.publish, undefined);
});

test('the website step binds the widget colour and voice and writes back what it reads', () => {
	const spec: Spec = { profile: { display: 'Ada' } };
	writeColor(spec, '#0B6BCB');
	assert.equal(readColor(throughEditor(spec)), '#0b6bcb');
	writeColor(spec, 'blue');
	assert.deepEqual(spec.profile, { display: 'Ada' }, 'a non-colour clears it');

	const voice = { input: true, output: true, voice: 'nova', transcriptionModel: 'stt', speechModel: 'tts' };
	writeVoice(spec, voice);
	assert.deepEqual(spec.publish.voice, { input: true, output: true, voice: 'nova', transcription_model: 'stt', speech_model: 'tts' });
	assert.deepEqual(readVoice(throughEditor(spec)), voice);
	assert.deepEqual(voiceMissing({ ...voice, speechModel: '' }, undefined), ['speech']);
	assert.deepEqual(voiceMissing({ ...voice, speechModel: '' }, { chat: null, transcription: null, speech: 'tts-1' }), [], 'the default serves it');
	assert.deepEqual(voiceMissing({ ...voice, input: false, transcriptionModel: '' }, undefined), []);

	writeVoice(spec, { input: false, output: false, voice: '', transcriptionModel: '', speechModel: '' });
	assert.equal(spec.publish, undefined, 'nothing chosen leaves no publish block');
	assert.equal(stepForPath('publish.voice.speech_model'), 'site');
	assert.equal(stepForPath('main.model'), 'basics');
	assert.equal(stepForPath('profile.color'), 'site');
	assert.equal(stepForPath('profile.display'), 'basics');
});

test('every template is a starter spec the steps read without leftovers', () => {
	for (const key of TEMPLATES) {
		const spec = templateSpec(key, tr);
		assert.ok(!JSON.stringify(spec).includes('"@'), `${key} kept a catalog key`);
		const b = readBasics(spec);
		assert.equal(b.extra, '', `${key}: every response line is a chip`);
		const h = readHandoffs(spec);
		assert.deepEqual(h.custom, [], `${key}: every route is a rule or the fallback`);
		assert.ok(readSlots(spec).every((r) => r.kind !== 'custom'), `${key}: every detail has a friendly kind`);
		const again = JSON.parse(JSON.stringify(spec));
		writeBasics(again, b);
		writeScope(again, readScope(spec));
		writeSlots(again, readSlots(spec));
		writeIdentity(again, readIdentity(spec), labels);
		writeHandoffs(again, h);
		writeSite(again, readSite(spec));
		writeColor(again, readColor(spec));
		writeVoice(again, readVoice(spec));
		assert.deepEqual(throughEditor(again), throughEditor(spec), `${key}: the steps write back what they read`);
	}
	assert.equal(templateSpec('support', tr).main.instructions.orchestration, '«agents-tpl-support-task»');
	assert.deepEqual(templateSpec('blank', tr), {});
});

test('a template keeps the name and the model already chosen', () => {
	const spec = applyTemplate({ profile: { display: 'Harald' }, main: { model: 'mid', tools: ['x'] } }, 'faq', tr);
	assert.equal(spec.profile.display, 'Harald');
	assert.equal(spec.main.model, 'mid');
	assert.equal(spec.main.tools, undefined);
	assert.ok(isBlank(ensureShape({ profile: { display: 'Harald' }, main: { model: 'mid' } })));
	assert.ok(!isBlank(spec));
});

test('the checklist points each open item at the step that fixes it', () => {
	const spec = templateSpec('support', tr);
	const todos = checklist(spec, [{ path: 'routes.fallback.human', message: 'bad' }, { path: 'finish', message: 'odd' }]);
	assert.deepEqual(
		todos.map((t) => [t.step, t.key ?? t.message, t.blocking]),
		[
			['basics', 'agents-setup-todo-model', true],
			['identity', 'agents-setup-todo-identity-connector', true],
			['site', 'agents-setup-todo-site', false],
			['routes', 'routes.fallback.human: bad', true],
			[null, 'finish: odd', true]
		]
	);
	assert.equal(sectionStatus('identity', spec, todos), 'open');
	assert.equal(sectionStatus('slots', spec, todos), 'done');
	assert.equal(sectionStatus('abilities', spec, todos), 'optional');
	assert.ok(
		!checklist(spec, [], { chat: 'qwen', transcription: null, speech: null }).some((t) => t.key === 'agents-setup-todo-model'),
		'an unset model runs on the gateway default'
	);

	writeScope(spec, { topics: [], refusal: '', strict: true });
	assert.ok(checklist(spec).some((t) => t.step === 'scope' && t.key === 'agents-setup-strict-needs'));
	assert.equal(stepForPath('state.topic.values'), 'routes');
	assert.equal(stepForPath('state.email'), 'slots');
	assert.equal(stepForPath('main.tools[1]'), 'abilities');
});

test('each section sums itself up in one line', () => {
	const spec = templateSpec('support', tr);
	writeScope(spec, { topics: ['Ceph', 'Licences'], refusal: 'No.', strict: true });
	writeHandoffs(spec, { rules: [{ route: null, topic: 'Invoices', details: false, identity: false, target: { kind: 'agent', id: 'b1' }, bind: {} }], fallback: true, custom: [] });
	const ctx = {
		tr: (key: string, args?: Record<string, string | number>) => (args ? `${key}(${Object.values(args).join('|')})` : key),
		grants: [grant('rag_collection', '7')],
		resources: { ...resources, defaults: { chat: 'qwen', transcription: null, speech: null } },
		agents: [{ id: 'b1', name: 'billing', display: 'Billing' }]
	};
	assert.equal(summary('basics', spec, ctx), 'agents-setup-sum-basics(«agents-tpl-support-task»|agents-setup-model-default(qwen))');
	spec.main.model = 'mid';
	assert.equal(summary('basics', spec, ctx), 'agents-setup-sum-basics(«agents-tpl-support-task»|mid)');
	assert.equal(summary('scope', spec, ctx), 'agents-setup-sum-scope-strict(Ceph, Licences)');
	assert.equal(summary('abilities', spec, ctx), 'croit docs');
	assert.equal(summary('slots', spec, ctx), '«agents-tpl-slot-name», «agents-tpl-slot-email»');
	assert.equal(summary('identity', spec, ctx), 'agents-setup-identity-email_code');
	assert.equal(summary('routes', spec, ctx), 'Invoices → Billing · agents-setup-sum-routes-other(agents-setup-rule-person)');
	assert.equal(summary('routes', spec, { ...ctx, agents: [] }), 'Invoices → b1 · agents-setup-sum-routes-other(agents-setup-rule-person)', 'an agent not shared with the viewer: its id');
	assert.equal(summary('site', spec, ctx), 'agents-setup-sum-site-none');
});

test('a proposal maps onto the steps: identity cards, friendly details, hand-off sentences', () => {
	assert.equal(suggestedMethod('website_login'), 'signed_in');
	assert.equal(suggestedMethod('lookup'), 'customer_lookup');
	assert.equal(suggestedMethod('carrier_pigeon'), null);

	const existing: SlotRow[] = [{ key: 'email', label: 'E-mail', kind: 'email', values: [] }];
	const rows = suggestedSlotRows(
		[
			{ name: 'email', label: 'E-mail', def: { type: 'email', set_by: ['llm'] } },
			{ name: 'order_number', label: 'Order number', def: { type: 'string', max_length: 500, set_by: ['llm'] } },
			{ name: 'quantity', label: 'Quantity', def: { type: 'integer', set_by: ['llm'] } },
			{ name: 'plan', label: 'Plan', def: { type: 'enum', values: ['basic', 'pro'], set_by: ['llm'] } },
			{ name: 'topic', label: 'Topic', def: { type: 'string', set_by: ['llm'] } }
		],
		existing
	);
	assert.deepEqual(rows.map((r) => [r.key, r.kind, r.values]), [['order_number', 'text', []], ['quantity', 'whole_number', []], ['plan', 'choice', ['basic', 'pro']]]);
	const spec: Spec = {};
	writeSlots(spec, [...existing, ...rows]);
	assert.deepEqual(readSlots(throughEditor(spec)).map((r) => r.kind), ['email', 'text', 'whole_number', 'choice']);

	const rules = suggestedRules([{ topic: 'Invoices', target: 'b1' }, { topic: 'invoices', target: 'human' }, { topic: 'Returns', target: 'human', details: true }], []);
	assert.deepEqual(rules.map((r) => [r.topic, r.target, r.details]), [['Invoices', { kind: 'agent', id: 'b1' }, false], ['Returns', { kind: 'human' }, true]]);
	assert.deepEqual(suggestedRules([{ topic: 'Returns', target: 'human' }], rules), []);
});

test('a proposed tone selects its chips by id and keeps only the rest as free text', () => {
	const current = { tones: ['detailed' as const], language: null, extra: 'old' };
	assert.deepEqual(suggestedTone({ chips: ['formal', 'friendly', 'freundlich'], language: 'de', response: 'Sign as Lena.' }, current), {
		tones: ['friendly', 'formal'],
		language: 'de',
		extra: 'Sign as Lena.'
	});
	assert.deepEqual(suggestedTone({ chips: [], language: null, response: `${TONE_LINES.brief}\nAnswer in the language the visitor writes in.` }, current), {
		tones: ['brief'],
		language: 'visitor',
		extra: ''
	});
	assert.equal(suggestedTone({ chips: ['brief'], language: null, response: '' }, { ...current, language: 'fr' }).language, 'fr');
});

test('a slot is named by its label, the managed ones from the catalog, the rest by their key as it is', () => {
	const spec: Spec = { state: { speicher_groesse: { type: 'string', description: 'Speichergröße' }, firma_name: { type: 'string' }, topic: { type: 'enum', description: 'What the request is about.' } } };
	assert.equal(slotLabel(spec, 'speicher_groesse', tr), 'Speichergröße');
	assert.equal(slotLabel(spec, 'firma_name', tr), 'firma_name');
	assert.equal(slotLabel(spec, 'topic', tr), '«agents-slot-label-topic»');
	assert.equal(slotLabel(spec, 'gone', tr), 'gone');
});

test('details keep the order they were given, and unordered slots follow by name', () => {
	const spec: Spec = {
		state: {
			zeta: { type: 'string', set_by: ['llm'] },
			alpha: { type: 'string', set_by: ['llm'] },
			second: { ...slotDef('text', 'Second'), order: 1 },
			first: { ...slotDef('email', 'First'), order: 0 }
		}
	};
	assert.deepEqual(readSlots(throughEditor(spec)).map((r) => r.key), ['first', 'second', 'alpha', 'zeta']);
	const rows = readSlots(spec);
	writeSlots(spec, [rows[1], rows[0], ...rows.slice(2)]);
	assert.deepEqual(readSlots(throughEditor(spec)).map((r) => r.key), ['second', 'first', 'alpha', 'zeta']);
	assert.equal(slotKind(spec.state.first), 'email', 'the order is not part of the friendly kind');
});

test('a voice direction is offered when the manager may grant a model of its kind, or one is named', () => {
	const r: AgentResources = { ...resources, models: { chat: [], transcription: [{ id: 'whisper', gdpr: true, nda: true }], speech: [] } };
	assert.equal(voiceOffered('speech', '', r), false);
	assert.equal(voiceOffered('speech', 'tts-old', r), true, 'a model already named can still be switched off');
	assert.equal(voiceOffered('transcription', '', r), true);
});

test('setup errors are plain words, never a raw HTTP status line', () => {
	const words = (key: string, args?: Record<string, string | number>) => (args ? `${key}:${JSON.stringify(args)}` : key);
	const raw = (status: number) => ({ status, message: `${status} Method Not Allowed`, issues: [] });
	for (const status of [404, 405, 501, 503]) {
		assert.equal(setupErrorMessage(raw(status), words, 'assist'), 'agents-error-assist-unavailable');
	}
	assert.equal(setupErrorMessage(raw(405), words), 'agents-error-generic');
	assert.equal(setupErrorMessage({ status: 0, message: 'Failed to fetch', issues: [] }, words, 'assist'), 'agents-error-network');
	assert.equal(setupErrorMessage({ ...raw(429), retryAfter: 42 }, words, 'assist'), 'agents-error-rate-retry:{"seconds":42}');
	assert.equal(setupErrorMessage(raw(429), words), 'agents-error-rate');
	assert.equal(setupErrorMessage(raw(502), words, 'assist'), 'agents-error-assist-failed');
	const told = { status: 403, code: 'assist_model_not_allowed', message: 'you may not use model x — pick one you hold', issues: [] };
	assert.equal(setupErrorMessage(told, words, 'assist'), told.message, "the server's own advice is kept");
	assert.equal(setupErrorMessage({ status: 500, code: 'x', message: '500 Internal Server Error', issues: [] }, words), 'agents-error-generic');
});

test('a draft the agent architect wrote reads as hand-off sentences and friendly details, not "advanced"', () => {
	// Written by `apply_changes` on the server; `review/tests.rs` pins it.
	const draft = JSON.parse(readFileSync(new URL('./fixtures/architect-draft.json', import.meta.url), 'utf8')) as Spec;
	const h = readHandoffs(throughEditor(draft));
	assert.deepEqual(h.custom, []);
	assert.equal(h.fallback, true);
	assert.deepEqual(
		h.rules.map((r) => [r.topic, r.target, r.identity]),
		[
			['Invoices', { kind: 'agent', id: 'billing-agent' }, false],
			['Complaints', { kind: 'human' }, false]
		]
	);
	assert.deepEqual(
		readSlots(throughEditor(draft)).map((r) => [r.key, r.kind]),
		[
			['order', 'text'],
			['issue', 'choice']
		]
	);
	const again = structuredClone(draft);
	writeHandoffs(again, h);
	assert.deepEqual(sortKeys(again.routes), sortKeys(draft.routes), 'writing the rules back changes nothing');
	assert.deepEqual(sortKeys(again.state), sortKeys(draft.state));
});

test('publishState tells blocked, recommended-only and ready apart', () => {
	const blocking = { step: 'basics', key: 'k', blocking: true } as const;
	const advice = { step: 'site', key: 'k', blocking: false } as const;
	assert.equal(publishState([]), 'ready');
	assert.equal(publishState([advice]), 'recommended');
	assert.equal(publishState([advice, blocking]), 'blocked');
});

test('the voice picker lists the speech model\'s own voices', () => {
	const r: AgentResources = {
		...resources,
		models: { chat: [], transcription: [], speech: [{ id: 'tts-1', gdpr: true, nda: true, voices: ['alloy', 'onyx'] }, { id: 'kokoro', gdpr: true, nda: true, voices: ['af_heart'] }] },
		defaults: { chat: null, transcription: null, speech: 'kokoro' }
	};
	const voice = { input: false, output: true, voice: '', transcriptionModel: '', speechModel: 'tts-1' };
	assert.deepEqual(speechVoices(voice, r), ['alloy', 'onyx']);
	assert.deepEqual(speechVoices({ ...voice, speechModel: '' }, r), ['af_heart'], 'unset: the default model\'s');
	assert.deepEqual(speechVoices({ ...voice, voice: 'nova' }, r), ['alloy', 'onyx', 'nova'], 'what is set stays visible');
	assert.deepEqual(speechVoices({ ...voice, speechModel: 'gone' }, r), []);
});

test('each person route keeps where it announces through an unrelated edit', () => {
	const spec: Spec = {};
	const person = (topic: string) => ({ route: null, topic, details: true, identity: false, target: { kind: 'human' as const }, bind: {} });
	writeSlots(spec, [{ key: '', label: 'E-mail', kind: 'email', values: [], fresh: true }]);
	writeHandoffs(spec, { rules: [person('Billing'), person('Tech')], fallback: true, custom: [] });
	spec.routes.billing.human = { notify: ['slack'] };
	const before = { billing: structuredClone(spec.routes.billing.human), tech: structuredClone(spec.routes.tech.human), fallback: structuredClone(spec.routes.fallback.human) };

	const read = readHandoffs(throughEditor(spec));
	writeHandoffs(spec, { ...read, rules: [...read.rules, person('Sales')] });
	writeSlots(spec, [...readSlots(spec), { key: '', label: 'Phone', kind: 'phone', values: [], fresh: true }]);

	assert.deepEqual(spec.routes.billing.human, before.billing);
	assert.deepEqual(spec.routes.tech.human, before.tech, 'every channel stays every channel');
	assert.deepEqual(spec.routes.fallback.human, before.fallback);
	assert.equal(sharedNotify(readHandoffs(spec)), undefined, 'the routes differ: no shared choice');
});

test('one choice for every person route only while they agree', () => {
	const spec: Spec = {};
	const person = { route: null, topic: 'Billing', details: false, identity: false, target: { kind: 'human' as const }, bind: {} };
	writeHandoffs(spec, { rules: [person], fallback: true, custom: [] });
	const h = readHandoffs(spec);
	assert.equal(sharedNotify(h), null, 'all of them: every channel');
	writeHandoffs(spec, notifyEverywhere(h, ['slack']));
	assert.deepEqual(spec.routes.billing.human, { notify: ['slack'] });
	assert.deepEqual(spec.routes.fallback.human, { notify: ['slack'] });
	assert.deepEqual(sharedNotify(readHandoffs(throughEditor(spec))), ['slack']);

	spec.routes.billing.human.inbox = 'support';
	writeHandoffs(spec, notifyEverywhere(readHandoffs(spec), null));
	assert.deepEqual(spec.routes.billing.human, { inbox: 'support' }, 'every channel: no list, the rest of the route kept');
	assert.deepEqual(spec.routes.fallback.human, {});
});

test('the notification choice covers what exists, and all of it is every channel', () => {
	const available = ['push', 'slack'];
	assert.deepEqual(notifyOn(null, available), ['push', 'slack'], 'unset: everything announces');
	assert.deepEqual(notifyOn(['slack', 'discord'], available), ['slack']);
	assert.deepEqual(setNotify(null, available, 'push', false), ['slack']);
	assert.equal(setNotify(['slack'], available, 'push', true), null, 'everything on again: every channel');
	assert.deepEqual(setNotify(['slack'], available, 'slack', false), []);
});

test('organization setup reuses ordinary steps while suppressing website provisioning', async () => {
    const { visibleSetupSteps, STEPS } = await import('./agent-setup.ts');
    const organization = { dot: { owner: { kind: 'organization' as const, id: 'actual-org', name: 'Acme Support' }, name: 'Harald', active: true } };
    assert.deepEqual(visibleSetupSteps({ dot: null }), STEPS);
    assert.ok(!visibleSetupSteps(organization).includes('site'));
    assert.ok(visibleSetupSteps(organization).includes('identity'));
    assert.ok(visibleSetupSteps(organization).includes('review'));
    assert.deepEqual(visibleSetupSteps(null), []);
});

test('organization checklist hides website suggestions but retains blocking authored website issues', () => {
    const agent = { dot: { owner: { kind: 'organization' as const, id: 'org', name: 'Acme Support' }, name: 'Harald', active: true } };
    const todos = checklist({ main: { model: 'actual-model', instructions: { orchestration: 'Actual task' } } }, [{ path: 'publish.origins', message: 'Actual invalid authored origin' }], null, agent);
    assert.ok(!todos.some((todo) => todo.key === 'agents-setup-todo-site'));
    assert.ok(todos.some((todo) => todo.message === 'publish.origins: Actual invalid authored origin' && todo.blocking && todo.step === null));
});
