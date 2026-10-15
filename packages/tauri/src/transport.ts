import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { OperationSnapshot } from './generated/contract.ts';

export type Cleanup = () => void;

/** Narrow IPC seam: no window, framework, or application state is required. */
export interface Transport {
  invoke<T>(command: string, args: { request: unknown }): Promise<T>;
  listen(event: string, callback: (snapshot: OperationSnapshot) => void): Promise<Cleanup>;
}

/** Injectable Tauri primitives also allow adapter tests without a WebView. */
export interface TauriAdapterDependencies {
  invoke: typeof invoke;
  listen: typeof listen;
}

export function createTauriTransport(
  dependencies: TauriAdapterDependencies = { invoke, listen },
): Transport {
  return {
    invoke<T>(command: string, args: { request: unknown }): Promise<T> {
      return dependencies.invoke<T>(command, args);
    },
    listen(event, callback) {
      return dependencies.listen<OperationSnapshot>(event, message => callback(message.payload));
    },
  };
}
