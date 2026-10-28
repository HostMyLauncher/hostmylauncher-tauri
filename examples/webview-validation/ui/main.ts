import { createClient } from '@hostmylauncher/tauri';

const output = document.querySelector<HTMLElement>('#status');
if (!output) throw new Error('Missing proof status element');

const client = createClient();
const show = (value: unknown) => { output.textContent = JSON.stringify(value, null, 2); };
let cleanup = () => {};

async function prove(): Promise<void> {
  let operationId: string | undefined;
  let downloadProgressSeen = false;
  let resolveTerminal!: (snapshot: Awaited<ReturnType<typeof client.getOperation>>) => void;
  let rejectTerminal!: (error: unknown) => void;
  const terminalEvent = new Promise<Awaited<ReturnType<typeof client.getOperation>>>((resolve, reject) => {
    resolveTerminal = resolve;
    rejectTerminal = reject;
  });
  // The event subscription begins before the query/start invokes. This is an
  // event-only observation; the explicit getOperation below proves recovery.
  void terminalEvent.catch(() => {});
  const timeout = window.setTimeout(
    () => rejectTerminal(new Error('Timed out waiting for terminal operation event')),
    15_000,
  );
  cleanup = client.subscribeOperations(snapshot => {
    if (!operationId || snapshot.operationId !== operationId) return;
    if (snapshot.progress?.stage === 'download' && (snapshot.progress.current ?? 0) > 0) {
      downloadProgressSeen = true;
    }
    if (snapshot.status === 'succeeded' || snapshot.status === 'failed') {
      resolveTerminal(snapshot);
    }
  }, rejectTerminal);

  try {
    const instance = await client.getInstance({
      launcherId: 'proof',
      instanceId: 'proof-instance',
    });
    const accepted = await client.startOperation({
      launcherId: 'proof',
      instanceId: 'proof-instance',
      kind: 'sync',
      code: null,
    });
    operationId = accepted.operationId;
    const terminal = await terminalEvent;
    const recovered = await client.getOperation({ operationId });
    if (terminal.status !== 'succeeded') {
      throw new Error(`Operation failed: ${JSON.stringify(terminal.error)}`);
    }
    if (terminal.instanceId !== 'proof-instance' || recovered.status !== 'succeeded') {
      throw new Error('Operation correlation/recovery mismatch');
    }
    if (!terminal.result || terminal.result.kind !== 'sync') {
      throw new Error('Expected successful sync result');
    }
    if (!downloadProgressSeen) throw new Error('No download progress event was received');

    const report = {
      ok: true,
      webview: true,
      query: instance.id,
      accepted: accepted.status,
      downloadProgressEvent: downloadProgressSeen,
      terminalEvent: terminal.status,
      recovered: recovered.status,
      result: terminal.result,
      subscriptionCleanupCalled: true,
    };
    cleanup();
    cleanup = () => {};
    show(report);
    await window.__TAURI__.core.invoke('finish_proof', { report: JSON.stringify(report) });
  } catch (error) {
    cleanup();
    cleanup = () => {};
    const report = { ok: false, webview: true, error: String(error?.stack ?? error) };
    show(report);
    try {
      await window.__TAURI__.core.invoke('finish_proof', { report: JSON.stringify(report) });
    } catch {
      // The process timeout still yields a failed validation result.
    }
  } finally {
    window.clearTimeout(timeout);
    cleanup();
  }
}

void prove();
