// Consume the built public TypeScript package through a headless line transport.
import { createClient } from '../../packages/tauri/dist/index.js';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';

export async function runExample() {
  const binary = fileURLToPath(new URL('../../target/debug/bridge-minimal-consumer', import.meta.url));
  const child = spawn(binary, [], { stdio: ['pipe', 'pipe', 'pipe'], env: process.env });
  const pending = new Map(), listeners = new Map();
  let nextId = 0, stderr = '';
  let stop = () => {};
  child.stderr.on('data', chunk => { stderr += chunk; });
  const lines = createInterface({ input: child.stdout });
  let processFailure;
  const fail = error => {
    processFailure = error;
    for (const { reject } of pending.values()) reject(error);
    pending.clear();
  };
  child.on('error', fail);
  const exited = once(child, 'exit');
  lines.on('line', line => {
    let message;
    try { message = JSON.parse(line); } catch (error) { fail(error); return; }
    if (message.event) {
      for (const callback of listeners.get(message.event) ?? []) callback(message.payload);
    } else {
      const call = pending.get(message.id);
      if (!call) return;
      pending.delete(message.id);
      if (message.error) call.reject(message.error); else call.resolve(message.result);
    }
  });
  child.on('exit', code => fail(new Error(`demo backend exited ${code}: ${stderr}`)));
  const transport = {
    invoke(command, args) {
      if (processFailure) return Promise.reject(processFailure);
      const id = ++nextId;
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        child.stdin.write(JSON.stringify({ id, command, args }) + '\n', error => {
          if (error) { pending.delete(id); reject(error); }
        });
      });
    },
    async listen(event, callback) {
      const callbacks = listeners.get(event) ?? new Set();
      listeners.set(event, callbacks); callbacks.add(callback);
      return () => { callbacks.delete(callback); if (!callbacks.size) listeners.delete(event); };
    },
  };
  const timer = setTimeout(() => child.kill(), 10_000);
  // Handle process-spawn failures even before the finally block awaits exit.
  exited.catch(() => {});
  try {
    const client = createClient(transport);
    const instance = await client.getInstance({ launcherId: 'demo', instanceId: 'demo' });
    const accepted = await client.startOperation({ launcherId: 'demo', instanceId: 'demo', kind: 'verify', code: null });
    let observed = 0;
    const completed = await Promise.race([
      new Promise((resolve, reject) => {
        stop = client.watchOperation(accepted.operationId, snapshot => {
          observed++;
          if (snapshot.status === 'succeeded' || snapshot.status === 'failed') resolve(snapshot);
        }, reject);
      }),
      exited.then(([code, signal]) => {
        throw new Error(`backend exited before terminal state (${code}, ${signal}): ${stderr}`);
      }),
    ]);
    return { instance, accepted, completed, observed };
  } finally {
    stop();
    child.stdin.end();
    try {
      const [code, signal] = await exited;
      if (code !== 0) throw new Error(`backend exit (${code}, ${signal}): ${stderr}`);
    } finally {
      lines.close();
      clearTimeout(timer);
    }
  }
}
