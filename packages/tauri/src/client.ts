import { COMMANDS, OPERATION_EVENT } from './generated/contract.ts';
import type {
  FileDescriptorDto,
  GetInstanceFilesRequest,
  GetInstanceRequest,
  GetOperationRequest,
  InstanceDto,
  ListInstancesRequest,
  OperationSnapshot,
  ResolveByCodeRequest,
  StartOperationRequest,
} from './generated/contract.ts';
import { createTauriTransport } from './transport.ts';
import type { Cleanup, Transport } from './transport.ts';

export type OperationListener = (snapshot: OperationSnapshot) => void;
export type SubscriptionErrorHandler = (error: unknown) => void;

export interface Client {
  listInstances(request: ListInstancesRequest): Promise<InstanceDto[]>;
  getInstance(request: GetInstanceRequest): Promise<InstanceDto>;
  resolveByCode(request: ResolveByCodeRequest): Promise<InstanceDto>;
  getInstanceFiles(request: GetInstanceFilesRequest): Promise<FileDescriptorDto[]>;
  /** Resolves to acceptance, not operation completion or installation readiness. */
  startOperation(request: StartOperationRequest): Promise<OperationSnapshot>;
  getOperation(request: GetOperationRequest): Promise<OperationSnapshot>;
  subscribeOperations(onSnapshot: OperationListener, onError: SubscriptionErrorHandler): Cleanup;
  /** Listens first, then queries session state to recover missed events. */
  watchOperation(operationId: string, onSnapshot: OperationListener, onError: SubscriptionErrorHandler): Cleanup;
}

/** Uses the real Tauri adapter by default; inject Transport for non-WebView tests. */
export function createClient(transport: Transport = createTauriTransport()): Client {
  function getOperation(request: GetOperationRequest): Promise<OperationSnapshot> {
    return transport.invoke(COMMANDS.getOperation, { request });
  }

  function subscribe(
    onSnapshot: OperationListener,
    onError: SubscriptionErrorHandler,
    operationId?: string,
  ): Cleanup {
    let disposed = false;
    let unlisten: Cleanup | undefined;
    const latest = new Map<string, { sequence: number; terminal: boolean }>();

    function report(error: unknown): void {
      if (disposed) return;
      // A caller throwing from its error handler must not become an unhandled
      // promise rejection from asynchronous setup or recovery.
      try {
        onError(error);
      } catch (handlerError) {
        queueMicrotask(() => { throw handlerError; });
      }
    }

    function receive(snapshot: OperationSnapshot): void {
      if (disposed || (operationId !== undefined && snapshot.operationId !== operationId)) return;
      const previous = latest.get(snapshot.operationId);
      if (previous && (previous.terminal || snapshot.sequence <= previous.sequence)) return;
      latest.set(snapshot.operationId, {
        sequence: snapshot.sequence,
        terminal: snapshot.status === 'succeeded' || snapshot.status === 'failed',
      });
      onSnapshot(snapshot);
    }

    async function setup(): Promise<void> {
      try {
        const remove = await transport.listen(OPERATION_EVENT, receive);
        if (disposed) {
          remove();
          return;
        }
        unlisten = remove;
        // Await registration before querying: both responses use the same
        // sequence gate, so an older recovery response cannot rewind an event.
        if (operationId !== undefined) {
          const snapshot = await getOperation({ operationId });
          receive(snapshot);
        }
      } catch (error) {
        report(error);
      }
    }
    void setup();

    return () => {
      if (disposed) return;
      disposed = true;
      latest.clear();
      const remove = unlisten;
      unlisten = undefined;
      // This affects only the listener; never invokes a backend command.
      remove?.();
    };
  }

  return {
    listInstances: request => transport.invoke(COMMANDS.listInstances, { request }),
    getInstance: request => transport.invoke(COMMANDS.getInstance, { request }),
    resolveByCode: request => transport.invoke(COMMANDS.resolveByCode, { request }),
    getInstanceFiles: request => transport.invoke(COMMANDS.getInstanceFiles, { request }),
    startOperation: request => transport.invoke(COMMANDS.startOperation, { request }),
    getOperation,
    subscribeOperations: (onSnapshot, onError) => subscribe(onSnapshot, onError),
    watchOperation: (operationId, onSnapshot, onError) => subscribe(onSnapshot, onError, operationId),
  };
}
