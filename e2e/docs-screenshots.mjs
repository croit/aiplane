import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const origin = process.env.AIPLANE_URL ?? 'http://127.0.0.1:8090';
const seedLog = process.env.DOCS_SEED_LOG ?? '/tmp/aiplane-docs-dev-ui-new.log';
const cookie = fs.readFileSync(seedLog, 'utf8').match(/^    id=(.+)$/m)?.[1];
assert.ok(cookie, 'The isolated dev-ui log must contain its synthetic admin session.');

function findPlaywright(directory) {
    const candidate = path.join(directory, 'playwright/index.mjs');
    if (fs.existsSync(candidate)) return candidate;
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
        if (entry.isDirectory()) {
            const found = findPlaywright(path.join(directory, entry.name));
            if (found) return found;
        }
    }
}

const playwright = process.env.PLAYWRIGHT_DIR
    ? path.join(process.env.PLAYWRIGHT_DIR, 'index.mjs')
    : findPlaywright(path.join(os.homedir(), '.local/share/mise/installs/npm-playwright-cli'));
assert.ok(playwright, 'Install the mise Playwright tool before capturing screenshots.');
const { chromium } = await import(playwright);
const cache = path.join(os.homedir(), 'Library/Caches/ms-playwright');
let executablePath = process.env.CHROME_EXE;
if (!executablePath && fs.existsSync(cache)) {
    const revisions = fs.readdirSync(cache).filter((name) => /^chromium-\d+$/.test(name))
        .sort((a, b) => Number(b.split('-')[1]) - Number(a.split('-')[1]));
    for (const revision of revisions) {
        for (const arch of ['chrome-mac-arm64', 'chrome-mac-x64']) {
            const candidate = path.join(cache, revision, arch,
                'Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing');
            if (fs.existsSync(candidate)) { executablePath = candidate; break; }
        }
        if (executablePath) break;
    }
}
const browser = await chromium.launch({ executablePath, headless: true });
try {
    const context = await browser.newContext({
        viewport: { width: 1400, height: 950 }, deviceScaleFactor: 2,
        locale: 'en-US', colorScheme: 'dark'
    });
    await context.addCookies([
        { name: 'id', value: cookie, url: origin },
        { name: 'lang', value: 'en', url: origin }
    ]);
    const page = await context.newPage();
    const sessionResponse = await context.request.get(`${origin}/api/v0/chat/sessions`);
    assert.equal(sessionResponse.status(), 200, 'Use the synthetic fixture, not a personal session.');
    const sessionsBody = await sessionResponse.json();
    const sessions = Array.isArray(sessionsBody) ? sessionsBody : sessionsBody.sessions;
    assert.ok(Array.isArray(sessions) && sessions.length, 'Screenshot fixture needs seeded conversations.');
    const conversation = sessions.find((session) => session.title === 'Has order 48217 shipped?');
    const documentConversation = sessions.find((session) => session.title === 'Draft a project brief');
    assert.ok(conversation && documentConversation, 'Both named synthetic dev-ui conversations must exist.');
    await page.goto(`${origin}/agents`);
    const agentLink = page.locator('a.card[href^="/agents/"]').filter({ hasText: 'Billing helper' }).first();
    await agentLink.waitFor();
    const agentPath = await agentLink.getAttribute('href');
    const seeded = await seed(context);
    const fullPage = 'body';
    const captures = [
        { name: 'chat-overview', route: `/chat/${conversation.id}`, selector: '[data-chat-composer]', full: true },
        { name: 'chat-composer', route: `/chat/${conversation.id}`, selector: '[data-chat-composer]' },
        { name: 'upstream-pool', route: '/admin/models?tab=upstreams', selector: 'article.card', text: 'wiremock-chat' },
        { name: 'memory-preferences', route: '/settings/memory', selector: 'section.card', text: 'Preferences' },
        { name: 'token-card', route: '/settings/tokens', selector: 'li.card', text: 'Local laptop' },
        { name: 'browser-status', route: '/tools/browser', selector: 'section.card', text: 'Status' },
        { name: 'knowledge-collection', route: '/rag', selector: 'article.card', text: 'acme-api' },
        { name: 'scheduled-action', route: '/scheduled', selector: 'article.card', text: 'Weekly dependency report' },
        { name: 'content-guard', route: '/admin/settings?tab=access', selector: '[data-testid="settings-section-content_guard"]' },
        { name: 'agent-checklist', route: agentPath, selector: '[data-agent-tab] aside section.card' },
        { name: 'document-canvas', route: `/chat/${documentConversation.id}`, selector: 'aside[aria-label="Canvas"]', prepare: 'canvas' },
        { name: 'request-priority', route: '/admin/tokens', selector: fullPage, full: true },
        { name: 'request-priority-error', route: '/admin/models?tab=upstreams', selector: fullPage, full: true, prepare: 'priority-error' },
        { name: 'organizations-admin', route: '/admin/organizations', selector: fullPage, full: true },
        { name: 'organization-dots', route: `/organizations/${seeded.organization}`, selector: fullPage, full: true },
        { name: 'dot-management-permission', route: '/admin/groups', selector: 'article', text: 'engineering' },
        { name: 'dot-action-rules', route: `/agents/${seeded.harald}?tab=tools`, selector: '[role="tabpanel"] li details', text: 'Actions', prepare: 'action-rules', height: 1500 },
        { name: 'dot-communication', route: `/agents/${seeded.harald}?tab=communication`, selector: '[role="tabpanel"]:visible', text: 'Can ask' },
        { name: 'dot-scheduled-work', route: `/scheduled/new?dot_id=${seeded.harald}`, selector: 'fieldset', text: 'Run as' },
        { name: 'dot-organization-schedule', route: `/scheduled/new?principal_id=${seeded.team}`, selector: fullPage, full: true, prepare: 'organization-schedule' },
        { name: 'connectors', file: 'docs/img/connectors.png', route: '/admin/connectors', selector: fullPage, full: true },
        { name: 'connector-audit', file: 'docs/img/connector-audit.png', route: '/admin/connectors/discord/audit', selector: fullPage, full: true }
    ];
    fs.mkdirSync('docs/img/guide', { recursive: true });
    const manifestPath = 'docs/site-support/captures.json';
    // The e2e capture scenarios (DOCS_*_CAPTURE, see e2e/README.md) record
    // their own images here; this run replaces only the entries it shoots.
    const manifest = fs.existsSync(manifestPath) ? JSON.parse(fs.readFileSync(manifestPath, 'utf8')) : [];
    for (const capture of captures) {
        await page.setViewportSize({ width: 1400, height: capture.height ?? 950 });
        const response = await page.goto(origin + capture.route, { waitUntil: 'domcontentloaded' });
        assert.equal(response.status(), 200, capture.route);
        assert.ok(!page.url().includes('/login'), 'Screenshot session must be authenticated.');
        if (capture.prepare === 'canvas') {
            await page.locator('[data-chat-composer]').waitFor();
            if (!(await page.locator(capture.selector).isVisible())) {
                await page.getByTitle('Show / hide the document canvas').click();
            }
        }
        if (capture.prepare === 'priority-error') await showPriorityError(page);
        if (capture.prepare === 'action-rules') await showActionRules(page);
        if (capture.prepare === 'organization-schedule') await fillOrganizationSchedule(page);
        let target = page.locator(capture.selector);
        if (capture.text) target = target.filter({ hasText: capture.text });
        if (capture.has) target = target.filter({ has: page.locator(capture.has) });
        target = target.first();
        await target.waitFor({ state: 'visible' });
        await page.evaluate(() => document.fonts.ready);
        await target.scrollIntoViewIfNeeded();
        const output = capture.file ?? `docs/img/guide/${capture.name}.png`;
        if (capture.full) await page.screenshot({ path: output });
        else await target.screenshot({ path: output });
        const size = capture.full ? { width: 1400, height: 950 } : await target.boundingBox();
        const entry = { file: output, route: capture.route, selector: capture.selector,
            matchingText: capture.text, viewport: { width: 1400, height: capture.height ?? 950 }, scale: 2,
            width: Math.round(size.width * 2), height: Math.round(size.height * 2),
            data: 'Synthetic dev-ui fixture; actual application components.' };
        const index = manifest.findIndex((existing) => existing.file === output);
        if (index === -1) manifest.push(entry);
        else manifest[index] = entry;
        console.log(`${output}: ${entry.width}x${entry.height}`);
    }
    fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
} finally {
    await browser.close();
}

/** The state the agent and organization captures show, made through the real routes. */
async function seed(context) {
    async function api(method, url, data) {
        const response = await context.request.fetch(origin + url, { method, data });
        assert.ok(response.ok(), `${method} ${url}: ${response.status()} ${await response.text()}`);
        return response.status() === 204 ? null : response.json();
    }
    const groups = await api('GET', '/api/v0/admin/groups');
    const engineering = (Array.isArray(groups) ? groups : groups.groups).find((group) => group.name === 'engineering');
    await api('PUT', '/api/v0/admin/groups', { ...engineering, can_manage_agents: false, can_manage_dots: true });

    const organization = (await api('GET', '/api/v0/admin/organizations')).organizations
        .find((row) => row.name === 'Acme Support')?.id ?? (await api('POST', '/api/v0/admin/organizations', {
            name: 'Acme Support', description: 'Team assistants for support operations.', member_groups: ['admin']
        })).organization.id;
    let team = (await api('GET', `/api/v0/organizations/${organization}`)).dots
        .find((agent) => agent.display === 'Harald Support')?.id;
    if (!team) {
        team = (await api('POST', `/api/v0/organizations/${organization}/dots`, {
            name: 'Harald Support', description: 'Review support requests and prepare the next steps for approval.'
        })).agent.id;
        await api('POST', `/api/v0/system-principals/${team}/grants`, { kind: 'model', ref: 'demo-model' });
        await api('PUT', `/api/v0/agents/${team}/draft`, { spec: { main: { model: 'demo-model',
            instructions: { orchestration: 'Review support requests and prepare the next steps for approval.' } } } });
        await api('POST', `/api/v0/agents/${team}/publish`);
    }

    const owned = (await api('GET', '/api/v0/dots')).dots;
    const personal = async (name, description, task, topics) => {
        const existing = owned.find((dot) => dot.display === name);
        if (existing) return existing.id;
        const id = (await api('POST', '/api/v0/dots', { name, description })).dot.id;
        const profile = await api('GET', `/api/v0/dots/${id}/profile`);
        await api('PUT', `/api/v0/dots/${id}/profile`, { revision: profile.revision, display: name, description, task, tone: '', topics });
        return id;
    };
    const harald = await personal('Harald', 'Keeps support tickets visible and flags those nearing escalation.',
        'Watch the support queue every morning and flag tickets older than two days.', ['Support', 'Escalations']);
    const clara = await personal('Clara', 'Researches public documentation and cites its sources.',
        'Answer research questions from public documentation, with sources.', ['Documentation']);
    if (owned.some((dot) => dot.display === 'Harald')) return { organization, team, harald, clara };
    const access = await api('GET', `/api/v0/dots/${harald}/access`);
    const kept = access.tools.filter((tool) => tool.selected && tool.id !== 'browse_page')
        .map((tool) => ({ id: tool.id, permission: tool.permission, actions: tool.action_rules }));
    await api('PUT', `/api/v0/dots/${harald}/access`, { revision: access.revision, tools: [...kept,
        { id: 'browse_page', permission: 'always_ask', actions: [{ id: 'read', rule: 'allow' }, { id: 'fill', rule: 'deny' }] }] });
    const consent = await api('PUT', `/api/v0/dots/${harald}/communication`,
        { peer_id: clara, direction: 'outbound', enabled: true, expected_generation: null });
    const generation = consent.peers.find((peer) => peer.dot.id === clara).outgoing.generation;
    await api('PUT', `/api/v0/dots/${clara}/communication`,
        { peer_id: harald, direction: 'inbound', enabled: true, expected_generation: generation });
    return { organization, team, harald, clara };
}

/** The backend form refusing a priority mapping whose values are not strictly ordered. */
async function showPriorityError(page) {
    await page.getByRole('button', { name: 'Edit backend', exact: true }).first().click();
    const dialog = page.getByRole('dialog');
    await dialog.getByRole('combobox', { name: 'Backend prioritization' }).selectOption({ label: 'Custom mapping' });
    for (const level of ['Low', 'Medium', 'High']) await dialog.getByRole('spinbutton', { name: level, exact: true }).fill('0');
    await dialog.getByRole('button', { name: 'Save backend', exact: true }).click();
    await dialog.getByRole('alert').waitFor();
}

/** The browse_page row with its action rules open. */
async function showActionRules(page) {
    const panel = page.getByRole('tabpanel', { name: 'Tools', exact: true });
    await panel.getByRole('textbox').first().fill('browse_page');
    const row = panel.locator('li').filter({ has: page.locator('code:text-is("browse_page")') });
    await row.locator('details summary').filter({ hasText: 'Actions' }).click();
    // The tool list scrolls inside the tab; let it grow so the whole row is in the shot.
    await row.evaluate((element) => {
        for (let node = element.parentElement; node && node.getAttribute('role') !== 'tabpanel'; node = node.parentElement) {
            if (/(auto|scroll)/.test(getComputedStyle(node).overflowY)) {
                node.style.maxHeight = 'none';
                node.style.overflow = 'visible';
            }
        }
    });
}

/** A team agent's schedule with its published version chosen. */
async function fillOrganizationSchedule(page) {
    await page.getByRole('textbox', { name: 'Name', exact: true }).fill('Check support queue');
    const version = page.getByRole('combobox', { name: 'Published version', exact: true });
    const value = await version.locator('option:not([value=""])').first().getAttribute('value');
    await version.selectOption(value);
    await page.getByRole('textbox', { name: 'Prompt', exact: true }).fill('Summarize new support requests and flag the urgent ones.');
}

