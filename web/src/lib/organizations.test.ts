import assert from 'node:assert/strict';
import test from 'node:test';

const organization = () => ({ id: 'actual/org', name: 'Acme Support', description: 'Actual support group', revision: 'original', member_groups: ['engineering'] });

test('organization metadata draft retains real values independently of persisted bindings', async () => {
    const { organizationDraft, organizationChange } = await import('./organizations.ts');
    const original = organization();
    const draft = organizationDraft(original);
    assert.equal(organizationChange(original, draft), null);
    draft.member_groups.push('actual-selected-group');
    draft.name = 'Authored revised name';
    assert.deepEqual(original.member_groups, ['engineering']);
    assert.deepEqual(organizationChange(original, draft), { name: 'Authored revised name', description: original.description, member_groups: ['engineering', 'actual-selected-group'], expected_revision: 'original' });
});

test('organization no-op compares group membership without treating empty bindings as all users', async () => {
    const { organizationDraft, organizationChange } = await import('./organizations.ts');
    const original = { ...organization(), member_groups: ['engineering', 'support'] };
    const draft = { ...organizationDraft(original), member_groups: ['support', 'engineering'] };
    assert.equal(organizationChange(original, draft), null);
    assert.deepEqual(organizationChange(original, { ...draft, member_groups: [] }), { name: original.name, description: original.description, member_groups: [], expected_revision: 'original' });
});

test('organization stale revision preserves the submitted draft until explicit reload', async (context) => {
    const { adminOrganizationsApi, organizationDraft, organizationChange } = await import('./organizations.ts');
    const { ApiError } = await import('./api.ts');
    const original = organization();
    const draft = { ...organizationDraft(original), name: 'Unsaved actual choice', member_groups: ['removed-real-group'] };
    const fetchOriginal = globalThis.fetch;
    globalThis.fetch = async () => new Response(JSON.stringify({ error: { type: 'conflict', message: 'Reload organization settings.' } }), { status: 409 });
    context.after(() => globalThis.fetch = fetchOriginal);
    await assert.rejects(adminOrganizationsApi.save(original.id, organizationChange(original, draft)!), (error: unknown) => error instanceof ApiError && error.status === 409);
    assert.equal(original.revision, 'original');
    assert.equal(draft.name, 'Unsaved actual choice');
    assert.deepEqual(draft.member_groups, ['removed-real-group']);
    const reloaded = { ...original, revision: 'current', member_groups: [] };
    assert.deepEqual(organizationDraft(reloaded).member_groups, []);
});

test('organization API uses actual scoped routes and sends authored fields plus metadata CAS only', async (context) => {
    const { organizationsApi, adminOrganizationsApi } = await import('./organizations.ts');
    const fetchOriginal = globalThis.fetch;
    const calls: { path: string; method: string; body: unknown }[] = [];
    globalThis.fetch = async (input, init) => {
        calls.push({ path: String(input), method: init?.method ?? 'GET', body: init?.body ? JSON.parse(String(init.body)) : null });
        return new Response(JSON.stringify({ organization: organization(), organizations: [], groups: [], dots: [], agent: { id: 'actual-dot' }, active_count: 1, active_limit: 4, can_create_dot: true }), { status: 200 });
    };
    context.after(() => globalThis.fetch = fetchOriginal);
    const body = { name: 'Acme Support', description: 'Authored', member_groups: ['engineering'] };
    await adminOrganizationsApi.list();
    await adminOrganizationsApi.create(body);
    await adminOrganizationsApi.save('actual/org', { ...body, expected_revision: 'current' });
    await organizationsApi.list();
    await organizationsApi.detail('actual/org');
    await organizationsApi.createDot('actual/org', { name: 'Harald', display: 'Harald Support', description: 'Authored task' });
    await organizationsApi.setActive('actual/org', 'actual/dot', false);
    assert.deepEqual(calls, [
        { path: '/api/v0/admin/organizations', method: 'GET', body: null },
        { path: '/api/v0/admin/organizations', method: 'POST', body },
        { path: '/api/v0/admin/organizations/actual%2Forg', method: 'PUT', body: { ...body, expected_revision: 'current' } },
        { path: '/api/v0/organizations', method: 'GET', body: null },
        { path: '/api/v0/organizations/actual%2Forg', method: 'GET', body: null },
        { path: '/api/v0/organizations/actual%2Forg/dots', method: 'POST', body: { name: 'Harald', display: 'Harald Support', description: 'Authored task' } },
        { path: '/api/v0/organizations/actual%2Forg/dots/actual%2Fdot/active', method: 'POST', body: { active: false } }
    ]);
});
