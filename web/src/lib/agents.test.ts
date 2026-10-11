import assert from 'node:assert/strict';
import test from 'node:test';

import { ApiError } from './api.ts';
import {
	grantOptions,
	gateHint,
	agentIdFromName,
	bindSource,
	cleanSpec,
	condKind,
	describeBindSource,
	embedSnippet,
	ensureShape,
	issuesAt,
	liveIssues,
	issuesUnder,
	parseBindSource,
	parseSpecError,
	renameKey,
	renameRoute,
	removeRoute,
	freshName,
	writerOptions,
	slotValueFromText,
	splitList,
	slotInfos,
	testTurnLabel,
	turnsToRead,
	settleAnswered,
	readFailure,
	shareSubjectOptions,
	shareSubjectLabel,
	type SpecIssue
} from './agents.ts';

const issues: SpecIssue[] = [
	{ path: 'main.model', message: 'model `x` is not granted' },
	{ path: 'main.tools[0]', message: 'tool `a` is not granted' },
	{ path: 'main.tools[1]', message: 'tool `b` is not granted' },
	{ path: 'routes.billing.when.all[1].slot', message: 'no slot `z`' },
	{ path: '', message: 'the spec is not an object' },
	{ path: 'main_other', message: 'unrelated' }
];

test('issues are found at a path exactly, or at it and below it', () => {
	assert.deepEqual(issuesAt(issues, 'main.model'), ['model `x` is not granted']);
	assert.deepEqual(issuesAt(issues, 'main'), []);
	assert.deepEqual(
		issuesUnder(issues, 'main.tools').map((i) => i.path),
		['main.tools[0]', 'main.tools[1]']
	);
	assert.deepEqual(
		issuesUnder(issues, 'routes.billing').map((i) => i.path),
		['routes.billing.when.all[1].slot']
	);
	assert.deepEqual(
		issuesUnder(issues, 'main').map((i) => i.path),
		['main.model', 'main.tools[0]', 'main.tools[1]'],
		'`main_other` is not below `main`'
	);
});

test('a 422 invalid_agent_spec becomes a list of path-keyed issues', () => {
	const body = JSON.stringify({
		error: {
			message: 'cannot save the draft: at `main.model`, bad',
			code: 'invalid_agent_spec',
			issues: [{ path: 'main.model', message: 'bad' }]
		}
	});
	const parsed = parseSpecError(new ApiError(422, `422 Unprocessable Entity — ${body}`, undefined, undefined, body));
	assert.equal(parsed.status, 422);
	assert.equal(parsed.code, 'invalid_agent_spec');
	assert.deepEqual(parsed.issues, [{ path: 'main.model', message: 'bad' }]);
	assert.match(parsed.message, /cannot save the draft/);
});

test('other failures keep the server message and carry no issues', () => {
	const body = JSON.stringify({ error: { message: 'you can read this agent', code: 'agent_write_required' } });
	const parsed = parseSpecError(new ApiError(403, `403 Forbidden — ${body}`, undefined, undefined, body));
	assert.equal(parsed.code, 'agent_write_required');
	assert.equal(parsed.message, 'you can read this agent');
	assert.deepEqual(parsed.issues, []);

	const plain = parseSpecError(new Error('network down'));
	assert.equal(plain.message, 'network down');
	assert.equal(plain.status, 0);
});

test('editing shape is filled in without disturbing what is there', () => {
	const shaped = ensureShape({ main: { model: 'p' }, extra: 1 });
	assert.deepEqual(shaped.main.tools, []);
	assert.deepEqual(shaped.main.skills, []);
	assert.deepEqual(shaped.main.instructions, { orchestration: '', response: '' });
	assert.deepEqual(shaped.state, {});
	assert.deepEqual(shaped.routes, {});
	assert.equal(shaped.main.model, 'p');
	assert.equal(shaped.extra, 1);
	assert.deepEqual(ensureShape({}).main.budget, {});
});

test('cleaning drops blanks but keeps what the validator must see', () => {
	const cleaned = cleanSpec({
		profile: { display: '', color: '' },
		main: {
			model: 'p',
			instructions: { orchestration: 'go', response: '' },
			tools: [],
			skills: [],
			tool_resources: { t: { bind: {}, permission: '' } },
			budget: { rounds: null, seconds: '', tokens: 5 }
		},
		state: { a: { type: '', set_by: [] }, b: { type: 'string', description: '' } },
		routes: { r: { when: { all: [] }, agent: 'x', task: '', bind: {} } },
		finish: { schema: { type: 'object', required: [] } },
		publish: { origins: [], idle_ttl: '' }
	});
	assert.deepEqual(cleaned, {
		main: {
			model: 'p',
			instructions: { orchestration: 'go' },
			tool_resources: { t: {} },
			budget: { tokens: 5 }
		},
		state: { a: {}, b: { type: 'string' } },
		routes: { r: { when: { all: [] }, agent: 'x' } },
		finish: { schema: { type: 'object', required: [] } }
	});
	assert.deepEqual(cleanSpec({}), { main: {} }, 'a spec always names its main agent');
});

test('cleaning does not mutate its input', () => {
	const input = { main: { model: '', tools: [] } };
	cleanSpec(input);
	assert.deepEqual(input, { main: { model: '', tools: [] } });
});

test('a gate node is a combinator or a leaf', () => {
	assert.equal(condKind({ all: [] }), 'all');
	assert.equal(condKind({ any: [] }), 'any');
	assert.equal(condKind({ not: { slot: 'a', set: true } }), 'not');
	assert.equal(condKind({ slot: 'a', eq: 1 }), 'leaf');
	assert.equal(condKind({}), 'leaf');
});

test('bind sources round trip between the form and the spec', () => {
	assert.equal(bindSource('state', 'verified.customer_id'), 'state.verified.customer_id');
	assert.equal(bindSource('route', 'customer'), 'route.customer');
	assert.deepEqual(bindSource('const', 'produktdoku'), { const: 'produktdoku' });
	assert.deepEqual(parseBindSource('state.verified.customer_id'), { kind: 'state', value: 'verified.customer_id' });
	assert.deepEqual(parseBindSource('route.customer'), { kind: 'route', value: 'customer' });
	assert.deepEqual(parseBindSource({ const: 7 }), { kind: 'const', value: '7' });
	assert.deepEqual(parseBindSource(undefined), { kind: 'const', value: '' });
	assert.equal(describeBindSource({ const: 'x' }), 'x');
	assert.equal(describeBindSource('route.a'), 'route.a');
});

test('a constant bind keeps a number a number', () => {
	assert.deepEqual(bindSource('const', '12'), { const: 12 });
	assert.deepEqual(bindSource('const', 'true'), { const: true });
	assert.deepEqual(bindSource('const', 'doc-1'), { const: 'doc-1' });
});

test('a gate value is read as the slot type says', () => {
	assert.equal(slotValueFromText('integer', '3'), 3);
	assert.equal(slotValueFromText('number', '2.5'), 2.5);
	assert.equal(slotValueFromText('boolean', 'true'), true);
	assert.equal(slotValueFromText('boolean', 'false'), false);
	assert.equal(slotValueFromText('enum', 'billing'), 'billing');
	assert.equal(slotValueFromText('integer', 'abc'), 'abc', 'left as typed so the validator can say why');
});

test('lists are typed comma or line separated', () => {
	assert.deepEqual(splitList('a, b\n c ,,'), ['a', 'b', 'c']);
	assert.deepEqual(splitList('  '), []);
});

test('a test turn is labelled by how it ended', () => {
	assert.equal(testTurnLabel('completed'), 'agents-test-status-completed');
	assert.equal(testTurnLabel('errored'), 'agents-test-status-errored');
	assert.equal(testTurnLabel('suspended'), 'agents-test-status-suspended');
	assert.equal(testTurnLabel('whatever'), 'agents-test-status-other');
});

test('a closed gate is said with the slot’s label, a condition without a slot in the server’s words', () => {
	const tr = (key: string, args?: Record<string, string | number>) => `${key}(${Object.entries(args ?? {}).filter(([, v]) => v !== '').map(([k, v]) => `${k}=${v}`).join(',')})`;
	assert.equal(gateHint({ path: 'all[0]', slot: 'topic', kind: 'missing', message: "`topic` is missing — call set_topic" }, 'Topic', tr), 'agents-gate-missing(slot=Topic)');
	assert.equal(
		gateHint({ path: 'all[0]', slot: 'topic', kind: 'not_equal', expected: 'Lead', message: '' }, 'Topic', tr),
		'agents-gate-not-equal(slot=Topic,expected=“Lead”)'
	);
	assert.equal(gateHint({ path: '', slot: 'plan', kind: 'not_in', expected: ['a', 1], message: '' }, 'Plan', tr), 'agents-gate-not-in(slot=Plan,expected=“a”, 1)');
	assert.equal(gateHint({ path: '', kind: 'unknown_route', message: 'there is no route `x`' }, 'x', tr), 'there is no route `x`');
});

test('renaming a key keeps its place and refuses a taken name', () => {
	assert.deepEqual(Object.keys(renameKey({ a: 1, b: 2, c: 3 }, 'b', 'x')), ['a', 'x', 'c']);
	assert.deepEqual(renameKey({ a: 1, b: 2 }, 'a', 'b'), { a: 1, b: 2 });
	assert.deepEqual(renameKey({ a: 1 }, 'zz', 'y'), { a: 1 });
});

test('renaming or removing a route keeps the router order naming routes that exist', () => {
	const spec = {
		routes: { billing: { agent: 'a' }, staff: { human: {} } },
		router: { kind: 'rules', order: ['billing', 'staff'] }
	};
	assert.equal(renameRoute(spec, 'staff', 'people'), 'people');
	assert.deepEqual(Object.keys(spec.routes), ['billing', 'people']);
	assert.deepEqual(spec.router.order, ['billing', 'people']);
	removeRoute(spec, 'billing');
	assert.deepEqual(spec.router.order, ['people']);
	removeRoute(spec, 'people');
	assert.deepEqual(spec.router, { kind: 'rules' }, 'an empty order is no order');
	assert.deepEqual(spec.routes, {});
});

test('the embed snippet loads the widget from the gateway with the new key', () => {
	assert.equal(
		embedSnippet('https://gw.example.com/embed.js', 'gwe_abc'),
		'<script src="https://gw.example.com/embed.js" data-agent-key="gwe_abc" async></script>'
	);
});

test('issues about an entry the draft no longer has are dropped, others kept', () => {
	const spec = {
		main: { tools: ['a'] },
		state: { issue: { type: 'enum' } },
		routes: { billing: { when: { slot: 'issue' } } },
		publish: { output_filter: { patterns: {} } }
	};
	const issues: SpecIssue[] = [
		{ path: 'routes.route_1.when.slot', message: 'gone route, deep' },
		{ path: 'routes.route_1', message: 'gone route' },
		{ path: 'state.topic', message: 'gone slot' },
		{ path: 'publish.output_filter.patterns.p1', message: 'gone pattern' },
		{ path: 'main.tools[3]', message: 'gone tool' },
		{ path: 'routes.billing.when.slot', message: 'still there' },
		{ path: 'state.issue.values', message: 'missing key of a live slot' },
		{ path: 'main.tools[0]', message: 'live tool' },
		{ path: '', message: 'root' }
	];
	assert.deepEqual(
		liveIssues(issues, spec).map((i) => i.message),
		['still there', 'missing key of a live slot', 'live tool', 'root']
	);
});

test('a fresh name skips the ones in use', () => {
	assert.equal(freshName({}, 'slot'), 'slot_1');
	assert.equal(freshName({ slot_1: 1, slot_2: 2 }, 'slot'), 'slot_3');
});

test('a slot may be written by the model, the host or a declared verifier', () => {
	assert.deepEqual(writerOptions({}), ['llm', 'host']);
	assert.deepEqual(writerOptions({ verifiers: { otp: {} } }), ['llm', 'host', 'verifier:otp']);
});

test('slot infos tell the gate editor each slot type, values and writers', () => {
	assert.deepEqual(
		slotInfos({
			state: {
				issue: { type: 'enum', values: ['billing', 'technical'], set_by: ['llm'] },
				verified: { type: 'subject', set_by: ['verifier:otp', 'host'] }
			}
		}),
		[
			{ name: 'issue', type: 'enum', values: ['billing', 'technical'], writers: ['llm'] },
			{ name: 'verified', type: 'subject', values: [], writers: ['verifier:otp', 'host'] }
		]
	);
	assert.deepEqual(slotInfos({}), []);
});

test('a reactive proxy can be cleaned and shaped like a plain object', () => {
	const proxy = new Proxy({ main: { model: 'p', tools: [] } }, {});
	assert.deepEqual(cleanSpec(proxy), { main: { model: 'p' } });
	assert.deepEqual(ensureShape(proxy).main.model, 'p');
});

test('an agent id is derived from the name people type', () => {
	assert.equal(agentIdFromName('Harald'), 'harald');
	assert.equal(agentIdFromName('Kundensupport Österreich'), 'kundensupport-oesterreich');
	assert.equal(agentIdFromName('  croit Support (Website)!  '), 'croit-support-website');
	assert.equal(agentIdFromName('Straße & Maß'), 'strasse-mass');
	assert.equal(agentIdFromName('Café Noël'), 'cafe-noel');
	assert.equal(agentIdFromName('支持'), 'agent');
	assert.equal(agentIdFromName('a'.repeat(80)).length, 48);
	assert.ok(!agentIdFromName(`${'a'.repeat(47)} b`).endsWith('-'));
});

test('a grant option is labelled by its title, with the reference when one item grants several', () => {
	const item = (key: string, title: string, refs: string[]) => ({
		key, kind: 'tool' as const, title, description: '', group: 'g', order: 0, icon: null,
		grant: { kind: 'tool' as const, refs }, tools: refs, editable: false, config_url: null
	});
	const resources = { items: [item('search_web', 'Web search', ['search_web']), item('memory', 'Memory', ['remember', 'recall'])] };
	assert.deepEqual(grantOptions(resources, 'tool'), [
		{ value: 'search_web', label: 'Web search' },
		{ value: 'remember', label: 'Memory: remember' },
		{ value: 'recall', label: 'Memory: recall' }
	]);
	assert.deepEqual(grantOptions(resources, 'skill'), []);
});

test('a share subject is picked from what the search found, by name where there is one', () => {
	const found = {
		truncated: false,
		users: [
			{ id: 'u1', name: 'Ada' },
			{ id: 'u2', name: null, email: 'sam@example.com' },
			{ id: 'u3', name: null }
		],
		groups: ['managers']
	};
	assert.deepEqual(shareSubjectOptions(found, 'user'), [
		{ value: 'u1', label: 'Ada' },
		{ value: 'u2', label: 'sam@example.com' },
		{ value: 'u3', label: 'u3' }
	]);
	assert.deepEqual(shareSubjectOptions(found, 'group'), [{ value: 'managers', label: 'managers' }]);
	assert.deepEqual(shareSubjectOptions(undefined, 'user'), []);
	assert.equal(shareSubjectLabel({ subject_id: 'u1', name: 'Ada' }), 'Ada');
	assert.equal(shareSubjectLabel({ subject_id: 'support' }), 'support');
});

test('the test chat reads each stopped answer once, never a running one or a message', () => {
	const live = (id: string, role: string, status: string) => ({ turn: { id, role, status } });
	const turns = [
		live('u1', 'user', 'completed'),
		live('a1', 'assistant', 'completed'),
		live('a2', 'assistant', 'suspended'),
		live('a3', 'assistant', 'in_progress')
	];
	assert.deepEqual(turnsToRead(turns, {}), ['a1', 'a2']);
	assert.deepEqual(turnsToRead(turns, { a1: {} }), ['a2']);
});

test('an answered pause is read again only once the stream shows it moved on', () => {
	const live = (id: string, status: string, request = 'r1') => ({
		turn: { id, role: 'assistant', status },
		suspension: status === 'suspended' ? { request_id: request } : null
	});
	const answered = { a2: 'r1' };
	const still = [live('a1', 'completed'), live('a2', 'suspended')];
	assert.deepEqual(settleAnswered(still, answered), answered, 'the stream has not reported the resume yet');
	assert.deepEqual(turnsToRead(still, { a1: {} }, answered), [], 'no debug request for a turn that is about to run');
	const running = [live('a1', 'completed'), live('a2', 'in_progress')];
	assert.deepEqual(settleAnswered(running, answered), {});
	assert.deepEqual(turnsToRead(running, { a1: {} }, {}), []);
	const done = [live('a1', 'completed'), live('a2', 'completed')];
	assert.deepEqual(settleAnswered(done, answered), {}, 'a resume that finished before the attach counts too');
	assert.deepEqual(turnsToRead(done, { a1: {} }, {}), ['a2']);
	const pausedAgain = [live('a2', 'suspended', 'r2')];
	assert.deepEqual(settleAnswered(pausedAgain, answered), {}, 'paused again on a new request');
	assert.deepEqual(turnsToRead(pausedAgain, {}, answered), ['a2']);
});

test('a debug view that is not ready yet is retried, any other failure is shown', () => {
	assert.equal(readFailure({ status: 409, code: 'turn_in_progress', message: 'running', issues: [] }), 'retry');
	assert.equal(readFailure({ status: 404, code: 'not_found', message: 'gone', issues: [] }), 'gone');
});

test('agent UI uses actual organization ownership for configuration-only capabilities', async () => {
    const { agentUiCapabilities } = await import('./agents.ts');
    const ordinary = { dot: null };
    const personal = { dot: { owner: { kind: 'user' as const, id: 'owner' }, name: 'Harald', active: true } };
    const organization = { dot: { owner: { kind: 'organization' as const, id: 'org', name: 'Acme Support' }, name: 'Harald', active: false } };
    assert.deepEqual(agentUiCapabilities(ordinary), { draftChat: true, execution: true, site: true, channels: true, embed: true });
    assert.deepEqual(agentUiCapabilities(personal), agentUiCapabilities(ordinary));
    assert.deepEqual(agentUiCapabilities(organization), { draftChat: false, execution: false, site: false, channels: false, embed: false });
    assert.deepEqual(agentUiCapabilities(null), { draftChat: false, execution: false, site: false, channels: false, embed: false });
});

test('organization draft chat requires actual current Write access without enabling published execution', async () => {
    const { agentUiCapabilities } = await import('./agents.ts');
    const dot = { owner: { kind: 'organization' as const, id: 'org', name: 'Acme Support' }, name: 'Harald', active: true };
    const writer = { dot, access: 'write' as const };
    const reader = { dot, access: 'read' as const };
    assert.deepEqual(agentUiCapabilities(writer), { draftChat: true, execution: false, site: false, channels: false, embed: false });
    assert.equal(agentUiCapabilities(reader).draftChat, false);
    assert.equal(agentUiCapabilities({ dot }).draftChat, false);
    assert.equal(agentUiCapabilities(undefined).draftChat, false);
    assert.equal(agentUiCapabilities({ ...writer, access: 'read' as const }).draftChat, false);
    assert.equal(agentUiCapabilities({ dot: null, access: 'read' as const }).draftChat, true);
});

test('agent owner presentation preserves authored Dot name and actual organization return path', async () => {
    const { organizationOwner, agentPublicName } = await import('./agents.ts');
    const org = { id: 'actual/org', name: 'Acme Support', kind: 'organization' as const };
    const dot = { name: 'internal-principal-id', display: 'Harald Support', dot: { owner: org, name: 'Harald', active: true } };
    assert.deepEqual(organizationOwner(dot), org);
    assert.equal(agentPublicName(dot), 'Harald');
    assert.equal(organizationOwner({ ...dot, dot: null }), null);
    assert.equal(agentPublicName({ ...dot, dot: null }), 'internal-principal-id');
});

test('agent resource API scopes the existing inventory by the actual target ID', async (context) => {
    const { agentsApi } = await import('./agents.ts');
    const fetchOriginal = globalThis.fetch;
    const paths: string[] = [];
    globalThis.fetch = async (input) => {
        paths.push(String(input));
        return new Response(JSON.stringify({ models: {}, defaults: {}, items: [] }), { status: 200 });
    };
    context.after(() => globalThis.fetch = fetchOriginal);
    await agentsApi.resources('actual/agent');
    await agentsApi.resources();
    assert.deepEqual(paths, ['/api/v0/agent-resources?agent_id=actual%2Fagent', '/api/v0/agent-resources']);
});

test('private test chat sends explicit draft and published sources and retains actual response versions', async (context) => {
    const { agentsApi } = await import('./agents.ts');
    const original = globalThis.fetch;
    const calls: { path: string; body: unknown }[] = [];
    globalThis.fetch = async (input, init) => {
        calls.push({ path: String(input), body: init?.body ? JSON.parse(String(init.body)) : null });
        const source = calls.length === 1 ? 'draft' : 'published';
        return new Response(JSON.stringify({ session_id: 'private', turn_id: 'answer', source, agent_version: source === 'draft' ? 0 : 3 }), { status: 202 });
    };
    context.after(() => globalThis.fetch = original);
    const draft = await agentsApi.sendTestMessage('harald', 'Draft task', null, 'draft');
    const published = await agentsApi.sendTestMessage('harald', 'Published task', null, 'published');
    const continued = await agentsApi.sendTestMessage('harald', 'Continue pinned task', published.session_id, published.source);
    assert.equal(draft.source, 'draft');
    assert.equal(draft.agent_version, 0);
    assert.equal(published.source, 'published');
    assert.equal(published.agent_version, 3);
    assert.equal(continued.agent_version, 3);
    assert.deepEqual(calls, [
        { path: '/api/v0/agents/harald/test/messages', body: { message: 'Draft task', source: 'draft' } },
        { path: '/api/v0/agents/harald/test/messages', body: { message: 'Published task', source: 'published' } },
        { path: '/api/v0/agents/harald/test/messages', body: { message: 'Continue pinned task', session_id: 'private', source: 'published' } }
    ]);
});

test('private test chat cancellation addresses the actual private session and turn', async (context) => {
    const { agentsApi } = await import('./agents.ts');
    const original = globalThis.fetch;
    let actual: unknown;
    globalThis.fetch = async (input, init) => {
        actual = { path: String(input), method: init?.method, body: JSON.parse(String(init?.body)) };
        return new Response(JSON.stringify({ ok: true }), { status: 200 });
    };
    context.after(() => globalThis.fetch = original);
    assert.deepEqual(await agentsApi.cancelTestConversation('principal/id', 'private/session', 'pending-turn'), { ok: true });
    assert.deepEqual(actual, { path: '/api/v0/agents/principal%2Fid/test/private%2Fsession/cancel', method: 'POST', body: { turn_id: 'pending-turn' } });
});

test('private test chat debug preserves actual published source and agent version', async (context) => {
    const { agentsApi } = await import('./agents.ts');
    const original = globalThis.fetch;
    const actual = { turn_id: 'answer', source: 'published', agent_version: 7, debug: { tool_calls: [] }, handoff: null };
    let path = '';
    globalThis.fetch = async (input) => {
        path = String(input);
        return new Response(JSON.stringify(actual), { status: 200 });
    };
    context.after(() => globalThis.fetch = original);
    assert.deepEqual(await agentsApi.testTurnView('harald', 'private', 'answer'), actual);
    assert.equal(path, '/api/v0/agents/harald/test/private/turns/answer/debug');
});

test('private test chat source choices require actual organization ownership and a published version', async () => {
    const { testChatSources } = await import('./agents.ts');
    const dot = { owner: { kind: 'organization' as const, id: 'org', name: 'Acme Support' }, name: 'Harald', active: true };
    assert.deepEqual(testChatSources({ dot, live_version: 3 }), ['draft', 'published']);
    assert.deepEqual(testChatSources({ dot, live_version: null }), ['draft']);
    assert.deepEqual(testChatSources({ dot, live_version: 0 }), ['draft']);
    assert.deepEqual(testChatSources({ dot: null, live_version: 3 }), ['draft']);
    assert.deepEqual(testChatSources(undefined), ['draft']);
});
