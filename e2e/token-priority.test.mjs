import { test } from 'node:test';
import assert from 'node:assert/strict';
import { BASE, devSessionCookie, launchBrowser } from './helpers.mjs';

test('owners create and edit token priority with cancellation, failed saves and a ceiling', async () => {
    const browser = await launchBrowser();
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, locale: 'en-US' });
    try {
        await context.addCookies([{ name: 'id', value: process.env.TOKEN_PRIORITY_COOKIE ?? await devSessionCookie(), url: BASE }]);
        const initial = await (await context.request.get(`${BASE}/api/v0/tokens/details`)).json();
        const name = `priority-e2e-${Date.now()}`;
        const page = await context.newPage();
        await page.goto(`${BASE}/settings/tokens`);
        const create = page.getByRole('button', { name: 'Create token', exact: true });
        await create.waitFor();
        await create.click();
        let dialog = page.locator('dialog[open]');
        await dialog.getByRole('textbox', { name: 'Name', exact: true }).fill(name);
        let select = dialog.getByRole('combobox', { name: 'Priority', exact: true });
        assert.equal(await select.locator('option[value="high"]').evaluate((option) => option.disabled), initial.max_priority !== 'high');
        await select.selectOption('low');
        await dialog.getByRole('button', { name: 'Create token', exact: true }).click();
        const card = page.locator('li.card').filter({ has: page.getByRole('heading', { name, exact: true }) });
        const tile = card.getByRole('button', { name: 'Priority · Edit', exact: true });
        await tile.getByText('Low', { exact: true }).waitFor();
        const details = await (await context.request.get(`${BASE}/api/v0/tokens/details`)).json();
        const token = details.tokens.find((token) => token.name === name);
        assert.equal(token.priority, 'low');
        assert.equal(details.max_priority, initial.max_priority);

        await tile.click();
        dialog = page.locator('dialog[open]');
        select = dialog.getByRole('combobox', { name: 'Priority', exact: true });
        await select.selectOption('medium');
        await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
        await tile.getByText('Low', { exact: true }).waitFor();
        await tile.click();
        assert.equal(await select.inputValue(), 'low');

        await page.route('**/api/v0/tokens/*/priority', (route) => route.fulfill({ status: 403, contentType: 'application/json', body: JSON.stringify({ error: { code: 'forbidden', message: 'Priority was refused.' } }) }), { times: 1 });
        await select.selectOption('medium');
        await dialog.getByRole('button', { name: 'Save', exact: true }).click();
        await dialog.getByRole('alert').getByText('Priority was refused.', { exact: false }).waitFor();
        assert.equal(await select.inputValue(), 'medium');
        const unchanged = await (await context.request.get(`${BASE}/api/v0/tokens/details`)).json();
        assert.equal(unchanged.tokens.find((row) => row.id === token.id).priority, 'low');
        await dialog.getByRole('button', { name: 'Save', exact: true }).click();
        await dialog.waitFor({ state: 'hidden' });
        await tile.getByText('Medium', { exact: true }).waitFor();
        await tile.click();
        await select.selectOption('');
        await dialog.getByRole('button', { name: 'Save', exact: true }).click();
        await dialog.waitFor({ state: 'hidden' });
        await tile.getByText('Inherit from groups (Medium)', { exact: true }).waitFor();
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);

        await create.click();
        dialog = page.locator('dialog[open]');
        await dialog.getByRole('textbox', { name: 'Name', exact: true }).fill('priority-failed-create');
        await page.route('**/api/v0/tokens', (route) => route.fulfill({ status: 403, contentType: 'application/json', body: JSON.stringify({ error: { code: 'forbidden', message: 'Priority was refused.' } }) }), { times: 1 });
        await dialog.getByRole('button', { name: 'Create token', exact: true }).click();
        await dialog.getByRole('alert').getByText('Priority was refused.', { exact: false }).waitFor();
        assert.equal(await dialog.getByRole('textbox', { name: 'Name', exact: true }).inputValue(), 'priority-failed-create');
    } finally {
        await context.close();
        await browser.close();
    }
});
