import test from 'node:test';
import assert from 'node:assert/strict';
import { runExample } from './client.mjs';

test('TypeScript client consumes the Rust bridge with a local provider', async () => {
  const summary = await runExample();
  assert.equal(summary.instance.id, 'demo');
  assert.equal(summary.accepted.status, 'queued');
  assert.equal(summary.completed.status, 'succeeded');
  assert.equal(summary.completed.kind, 'verify');
  assert.deepEqual(summary.completed.result, {
    kind: 'verify', checked: 1, issues: [{ path: 'mods/demo.jar', kind: 'missing' }],
  });
  assert.equal(summary.completed.error, null);
  assert.equal(summary.completed.operationId, summary.accepted.operationId);
  assert.ok(summary.completed.sequence > summary.accepted.sequence);
  assert.ok(summary.observed > 0);
});
