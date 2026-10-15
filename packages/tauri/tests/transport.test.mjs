import test from 'node:test';
import assert from 'node:assert/strict';
import { createTauriTransport } from '../src/transport.ts';

test('Tauri adapter forwards invoke arguments and unwraps event.payload only', async () => {
  const calls = [], received = [], response = { accepted: true };
  let handler, removed = 0;
  const transport = createTauriTransport({
    async invoke(command, args) { calls.push([command, args]); return response; },
    async listen(event, callback) {
      calls.push(['listen', event]); handler = callback;
      return () => { removed++; };
    },
  });
  const args = { request: { operationId: 'operation' } };
  assert.equal(await transport.invoke('registered-command', args), response);
  const stop = await transport.listen('registered-event', payload => received.push(payload));
  const payload = { operationId: 'operation', sequence: 2 };
  handler({ event: 'registered-event', id: 8, payload });
  assert.deepEqual(calls, [['registered-command', args], ['listen', 'registered-event']]);
  assert.deepEqual(received, [payload]); assert.equal(received[0], payload);
  stop(); assert.equal(removed, 1);
});

test('Tauri adapter preserves invoke and listener rejection', async () => {
  const failure = { code: 'internal' };
  const transport = createTauriTransport({
    async invoke() { throw failure; },
    async listen() { throw failure; },
  });
  await assert.rejects(transport.invoke('command', { request: {} }), error => error === failure);
  await assert.rejects(transport.listen('event', assert.fail), error => error === failure);
});
