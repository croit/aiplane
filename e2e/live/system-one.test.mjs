// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const base = process.env.SYSTEM_ONE_GATEWAY_URL ?? 'http://127.0.0.1:8080';
const credentials = process.env.SYSTEM_ONE_TOKEN_FILE
  ? JSON.parse(readFileSync(process.env.SYSTEM_ONE_TOKEN_FILE, 'utf8'))
  : {};
const token = process.env.SYSTEM_ONE_GATEWAY_TOKEN
  ?? credentials.bearer;
const sessionCookie = process.env.SYSTEM_ONE_SESSION_COOKIE ?? credentials.sessionCookie;
assert.ok(token, 'Set SYSTEM_ONE_GATEWAY_TOKEN or SYSTEM_ONE_TOKEN_FILE to a gateway user token');
const models = (process.env.SYSTEM_ONE_MODELS ?? 'typesafe/jev-1.13,~typesafe/jev-latest').split(',');

async function request(path, body) {
  const response = await fetch(new URL(path, base), {
    method: body ? 'POST' : 'GET',
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
    signal: AbortSignal.timeout(60_000),
  });
  assert.equal(response.status, 200, `${path} returned HTTP ${response.status}`);
  return response.json();
}

test('a user can discover and evaluate real System One models through the gateway', async (t) => {
  async function meteredTokens() {
    const url = new URL('/api/v0/usage?period=today', base);
    if (credentials.tokenId) url.searchParams.set('token', credentials.tokenId);
    const response = await fetch(url, {
      headers: { cookie: sessionCookie },
      signal: AbortSignal.timeout(10_000),
    });
    assert.equal(response.status, 200, 'usage requires a session belonging to the token owner');
    const usage = await response.json();
    assert.equal(usage.usage_enabled, true);
    return usage.summary.total_tokens;
  }
  const before = sessionCookie ? await meteredTokens() : undefined;
  let reportedTokens = 0;
  const listing = await request('/v1/models');
  for (const model of models) {
    await t.test(model, async () => {
      assert.ok(listing.data.some((entry) => entry.id === model), `${model} must be listed`);
      const detail = await request(`/v1/models/${encodeURIComponent(model)}`);
      assert.equal(detail.id, model);
      const result = await request('/v1/systemone', {
        model,
        state: 'My credit card was charged twice. Please refund the duplicate payment.',
        questions: {
          department: {
            type: 'choice',
            instructions: 'Which team should handle this ticket?',
            criteria: { billing: 'Payments and refunds', technical: 'Software bugs', sales: 'Product purchases' },
          },
          refund: { type: 'noul', instructions: 'Does the customer request a refund?' },
          urgency: { type: 'score', instructions: 'How urgent is this request?', criteria: ['Routine', 'Urgent', 'Emergency'] },
        },
      });
      assert.equal(typeof result.model, 'string');
      assert.equal(result.answers.department.type, 'choice');
      assert.equal(result.answers.department.choice, 'billing');
      assert.equal(result.answers.refund.type, 'noul');
      assert.ok(result.answers.refund.noul > 0.9 && result.answers.refund.noul <= 1);
      assert.equal(result.answers.urgency.type, 'score');
      assert.ok(result.answers.urgency.score >= 0 && result.answers.urgency.score <= 2);
      for (const answer of [result.answers.department, result.answers.urgency]) {
        const probabilities = Object.values(answer.probabilities);
        assert.ok(probabilities.every((p) => p >= 0 && p <= 1));
        assert.ok(Math.abs(probabilities.reduce((sum, p) => sum + p, 0) - 1) < 0.02);
        assert.ok(answer.confidence >= 0 && answer.confidence <= 1);
      }
      assert.ok(result.usage.input_tokens > 0);
      assert.ok(result.usage.output_tokens >= 0);
      reportedTokens += result.usage.input_tokens + result.usage.output_tokens;
      t.diagnostic(JSON.stringify({ requested: model, resolved: result.model, provider: result.provider, usage: result.usage }));
    });
  }
  await t.test('gateway usage records the tokens reported by the real upstream', { skip: !sessionCookie }, async () => {
    let recorded = 0;
    for (let attempt = 0; attempt < 20; attempt++) {
      recorded = await meteredTokens() - before;
      if (recorded >= reportedTokens) break;
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    assert.ok(recorded >= reportedTokens, `gateway recorded ${recorded} tokens; upstream reported ${reportedTokens}`);
  });
});
