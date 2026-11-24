import test from 'node:test';
import assert from 'node:assert/strict';
import { createClient } from '../src/client.ts';
import { CATALOG_EVENT, COMMANDS, OPERATION_EVENT } from '../src/generated/contract.ts';

const snapshot = (operationId = 'a', sequence = 0, status = 'queued') => ({
  operationId, launcherId: 'launcher', instanceId: 'instance', kind: 'sync', status, sequence,
  progress: null, result: null, error: null,
});
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
const tick = () => new Promise(resolve => setImmediate(resolve));
function fixture({ delayed = false } = {}) {
  const calls = [], setup = deferred();
  let listener, removed = 0;
  const transport = {
    async invoke(command, args) { calls.push([command, args]); return snapshot(); },
    listen(event, callback) {
      calls.push(['listen', event]); listener = callback;
      return delayed ? setup.promise : Promise.resolve(() => { removed++; });
    },
  };
  return { client: createClient(transport), calls, setup, transport,
    emit: value => listener(value), removed: () => removed,
    ready: () => setup.resolve(() => { removed++; }),
  };
}

test('generated identifiers match the registered wire contract', () => {
  assert.deepEqual(COMMANDS, {
    listInstances: 'plugin:hostmylauncher|list_instances',
    getInstance: 'plugin:hostmylauncher|get_instance',
    resolveByCode: 'plugin:hostmylauncher|resolve_by_code',
    getInstanceFiles: 'plugin:hostmylauncher|get_instance_files',
    startOperation: 'plugin:hostmylauncher|start_operation',
    getOperation: 'plugin:hostmylauncher|get_operation',
    watchCatalog: 'plugin:hostmylauncher|watch_catalog',
  });
  assert.equal(OPERATION_EVENT, 'hostmylauncher://operation');
  assert.equal(CATALOG_EVENT, 'hostmylauncher://catalog');
});

test('every command wraps its exact request; optional inputs stay explicit null', async () => {
  const f = fixture();
  await f.client.listInstances({ launcherId: 'launcher', limit: null, offset: 0 });
  await f.client.getInstance({ launcherId: 'launcher', instanceId: 'instance' });
  await f.client.resolveByCode({ launcherIds: ['launcher'], code: 'input-only' });
  await f.client.getInstanceFiles({ launcherId: 'launcher', instanceId: 'instance', code: null });
  const accepted = await f.client.startOperation({ launcherId: 'launcher', instanceId: 'instance', kind: 'repair', code: null });
  await f.client.getOperation({ operationId: 'a' });
  assert.deepEqual(f.calls, [
    [COMMANDS.listInstances, { request: { launcherId: 'launcher', limit: null, offset: 0 } }],
    [COMMANDS.getInstance, { request: { launcherId: 'launcher', instanceId: 'instance' } }],
    [COMMANDS.resolveByCode, { request: { launcherIds: ['launcher'], code: 'input-only' } }],
    [COMMANDS.getInstanceFiles, { request: { launcherId: 'launcher', instanceId: 'instance', code: null } }],
    [COMMANDS.startOperation, { request: { launcherId: 'launcher', instanceId: 'instance', kind: 'repair', code: null } }],
    [COMMANDS.getOperation, { request: { operationId: 'a' } }],
  ]);
  assert.equal(accepted.status, 'queued');
  assert.equal(accepted.result, null);
});

test('queries return projections and preserve structured rejection without inventing completion', async () => {
  const f = fixture();
  const result = [{ id: 'public' }];
  f.transport.invoke = async () => result;
  assert.equal(await f.client.listInstances({ launcherId: 'launcher', limit: null, offset: null }), result);
  const error = { code: 'operation_conflict', operationId: null, stage: null, context: null };
  f.transport.invoke = async () => { throw error; };
  await assert.rejects(f.client.startOperation({ launcherId: 'launcher', instanceId: 'instance', kind: 'verify', code: null }), value => value === error);
});

test('subscribe deduplicates per operation, preserves payloads and never revives terminal state', async () => {
  const f = fixture(), seen = [], errors = [];
  const stop = f.client.subscribeOperations(value => seen.push(value), error => errors.push(error));
  assert.equal(typeof stop, 'function');
  const running = { ...snapshot('a', 2, 'running'), progress: { stage: 'verify', current: null, total: null, unit: null } };
  const terminal = { ...snapshot('a', 3, 'succeeded'), result: { kind: 'verify', checked: 1, issues: [{ path: 'modified', kind: 'different' }] } };
  f.emit(running); f.emit(snapshot('a', 2)); f.emit(snapshot('a', 1));
  f.emit(snapshot('b', 0)); f.emit(terminal); f.emit(snapshot('a', 4, 'running'));
  assert.deepEqual(seen, [running, snapshot('b', 0), terminal]);
  stop(); stop(); await tick();
  f.emit(snapshot('c', 0));
  assert.equal(seen.length, 3); assert.equal(f.removed(), 1); assert.deepEqual(errors, []);
});

test('watch listens before recovery query and ignores stale response and unrelated events', async () => {
  const f = fixture({ delayed: true }), query = deferred(), seen = [];
  f.transport.invoke = (command, args) => { f.calls.push([command, args]); return query.promise; };
  const stop = f.client.watchOperation('a', value => seen.push(value), assert.fail);
  assert.deepEqual(f.calls, [['listen', OPERATION_EVENT]]);
  f.emit(snapshot('a', 4, 'running')); f.emit(snapshot('b', 5));
  f.ready(); await tick();
  assert.deepEqual(f.calls[1], [COMMANDS.getOperation, { request: { operationId: 'a' } }]);
  query.resolve(snapshot('a', 1)); await tick();
  assert.deepEqual(seen, [snapshot('a', 4, 'running')]);
  stop(); assert.equal(f.removed(), 1);
});

test('watch recovers missed terminal state by query', async () => {
  const f = fixture(), seen = [];
  const terminal = { ...snapshot('a', 7, 'failed'), error: { code: 'internal', operationId: 'a', stage: null, context: null } };
  f.transport.invoke = async () => terminal;
  const stop = f.client.watchOperation('a', value => seen.push(value), assert.fail);
  await tick(); assert.deepEqual(seen, [terminal]); stop();
});

for (const method of ['subscribeOperations', 'watchOperation']) {
  test(`${method}: synchronous early cleanup removes delayed listener without query or callbacks`, async () => {
    const f = fixture({ delayed: true }), seen = [], errors = [];
    const args = [value => seen.push(value), error => errors.push(error)];
    const stop = method === 'watchOperation' ? f.client[method]('a', ...args) : f.client[method](...args);
    stop(); stop(); f.emit(snapshot()); f.ready(); await tick(); f.emit(snapshot('a', 1));
    assert.equal(f.removed(), 1); assert.deepEqual(seen, []); assert.deepEqual(errors, []);
    assert.deepEqual(f.calls, [['listen', OPERATION_EVENT]]);
  });
  test(`${method}: asynchronous setup failure is explicitly reported`, async () => {
    const f = fixture({ delayed: true }), errors = [], failure = new Error('listen failed');
    const args = [assert.fail, error => errors.push(error)];
    const stop = method === 'watchOperation' ? f.client[method]('a', ...args) : f.client[method](...args);
    f.setup.reject(failure); await tick(); assert.deepEqual(errors, [failure]); stop();
  });
}

test('watch query failure is reported and disposal suppresses a pending query', async () => {
  const f = fixture(), query = deferred(), errors = [], seen = [];
  f.transport.invoke = () => query.promise;
  const stop = f.client.watchOperation('a', value => seen.push(value), error => errors.push(error));
  await tick(); stop(); query.resolve(snapshot()); await tick();
  assert.deepEqual(seen, []); assert.deepEqual(errors, []);
  f.transport.invoke = async () => { throw 'safe-query-error'; };
  const stop2 = f.client.watchOperation('a', assert.fail, error => errors.push(error));
  await tick(); assert.deepEqual(errors, ['safe-query-error']); stop2();
});

test('synchronous listener setup failure is reported without throwing from subscribe', () => {
  const f = fixture(), errors = [], failure = new Error('synchronous setup failure');
  f.transport.listen = () => { throw failure; };
  const stop = f.client.subscribeOperations(assert.fail, error => errors.push(error));
  assert.equal(typeof stop, 'function');
  assert.deepEqual(errors, [failure]); stop();
});

test('pending recovery rejection after disposal is handled without callbacks', async () => {
  const f = fixture(), query = deferred(), errors = [];
  f.transport.invoke = () => query.promise;
  const stop = f.client.watchOperation('a', assert.fail, error => errors.push(error));
  await tick(); stop(); query.reject(new Error('late recovery')); await tick();
  assert.deepEqual(errors, []); assert.equal(f.removed(), 1);
});

test('late setup rejection after disposal is handled without notifying caller', async () => {
  const f = fixture({ delayed: true }), errors = [];
  const stop = f.client.watchOperation('a', assert.fail, error => errors.push(error));
  stop(); f.setup.reject(new Error('late')); await tick(); assert.deepEqual(errors, []);
});

test('catalog subscription delivers snapshots until cleanup, even before registration', async () => {
  const f = fixture({ delayed: true }), seen = [];
  const catalog = { launcherId: 'launcher', instances: [] };
  const stop = f.client.subscribeCatalog(value => seen.push(value), assert.fail);
  assert.deepEqual(f.calls, [['listen', CATALOG_EVENT]]);
  f.emit(catalog); f.ready(); await tick();
  stop(); f.emit(catalog);
  assert.deepEqual(seen, [catalog]); assert.equal(f.removed(), 1);

  const early = fixture({ delayed: true });
  early.client.subscribeCatalog(assert.fail, assert.fail)();
  early.ready(); await tick(); early.emit(catalog);
  assert.equal(early.removed(), 1);
});
