# IPC contract v1

The Rust bridge is a Tauri 2 plugin named `hostmylauncher`. The TypeScript client
is framework-independent. Cancellation is not supported in v1. Unsubscribing or
closing a screen does not cancel an accepted operation.

## Trust boundary

The Rust host supplies its `hostmylauncher::Client`, allowed launcher catalog,
and per-instance registrations (launcher ID, destination, manifest path).
No IPC command accepts a destination, manifest path, arbitrary remote file URL,
base API URL, or bearer token. The bridge fetches provider files itself for operations.
Catalog queries must remain inside the host-configured catalog.

The local sibling dependency on `hostmylauncher-rs` is development-only. Both
packages remain non-publishable until reproducible dependency sourcing and a
license are selected. The base is never modified by this integration.

## Wire naming

Payload fields use camelCase. Enum values use snake_case. Optional values are
explicit JSON nulls, not omitted. Commands accept one argument named `request`.

The Rust contract is the source of truth for generated TypeScript DTOs and
command/event identifiers. The chosen generator and reproducible check commands
must be documented with the implementation; generated files are not hand-edited.

| IPC command | Request | Response |
| --- | --- | --- |
| `plugin:hostmylauncher\|list_instances` | launcherId, limit (nullable), offset (nullable) | InstanceDto[] |
| `plugin:hostmylauncher\|get_instance` | launcherId, instanceId | InstanceDto |
| `plugin:hostmylauncher\|resolve_by_code` | launcherIds (1–20), code | InstanceDto |
| `plugin:hostmylauncher\|get_instance_files` | launcherId, instanceId, code (nullable) | FileDescriptorDto[] |
| `plugin:hostmylauncher\|start_operation` | instanceId, kind, code (nullable) | OperationSnapshot |
| `plugin:hostmylauncher\|get_operation` | operationId | OperationSnapshot |

Query results are explicit projections, never serialization of base models with
flattened unknown fields. InstanceDto includes public ID/catalog/title/version/
loader/Java/RAM/artwork/locked/download metadata, but no embedded files, users,
access code or arbitrary extra fields. File descriptors expose provider relative
path/name, size and hashes, never download URLs.

## Operations

`kind`: `sync`, `verify`, `repair`. These are distinct intents, not a force flag.

```text
queued -> running -> succeeded
                  -> failed
```

`start_operation` returns the accepted queued snapshot and schedules the work in
Rust. Request/admission failures reject the command without creating operation
state, making provider requests, or touching files. Execution failures are stored
in the operation's terminal snapshot. A successful start response means accepted,
not successful installation.

Snapshots contain operationId, instanceId, kind, status, sequence, progress,
result and error. Sequence increases monotonically within one operation. Results
and errors are null before terminal state; terminal success contains result only,
terminal failure contains error only. State survives frontend remounts during the
backend session, not process exit. The host must not replace/drop the service
while work is active. Session history is not persistent installation state.

The event `hostmylauncher://operation` carries a complete OperationSnapshot.
Event delivery is best effort: query get_operation to recover authoritative state.
Consumers ignore duplicate/older sequences. Register the listener before querying
state to avoid a query/subscription gap. Callbacks never revive terminal operations.

Admission guards operate on filesystem identity, not merely instanceId. Existing
canonical aliases, ancestor/descendant roots and conflicting manifest resources
must not permit overlapping in-process operations. This cannot prevent another
process from modifying the installation; the launcher must ensure exclusive use.

## Progress

Progress contains stage and nullable current, total, unit. Units distinguish
`bytes`, `entries` and `archives`; zero/unknown byte totals become null. No overall
percentage is promised. Download callbacks describe one file/attempt and can reset
on retries; they occur before final integrity validation. Install counts describe
preparation of regular entries, not committed replacements. Extract describes one
staged archive. Remove is declared upstream but not currently emitted. Repair
provides only download callbacks; verification has no progress callback. Fetching
provider files and verification without a base callback are indeterminate. Stage
names include `fetching_files`, `download`, `extract`, `install`, `remove`, `verify`.

Success is recorded only after the library call returns, including applicable
manifest persistence. A generic error does not guarantee that rollback succeeded;
do not claim that every failure leaves the installation unchanged. It does not
mean Minecraft/Java/loader or the launcher's
full installation completed. Library rollback is not full-launcher rollback.

## Results

- Sync: downloaded and removed, matching SyncReport semantics. Downloaded includes
  staged provider comparisons; it does not necessarily mean installed files.
- Verify: checked and issues (relative path plus missing/different). Differences
  are a successful report. Unrelated local files are not integrity issues.
- Repair: pre-repair verification report, repaired and downloaded, matching
  RepairReport. Repaired user modifications are intentional. Unrelated and obsolete
  files are retained under the base contract.

These counters reflect the base, not additional inferred accounting.

## Errors

BridgeError has code, nullable operationId, nullable stage and nullable context.
Context carries only safe typed data, such as an HTTP status. No raw upstream
message, reqwest URL, access code, bearer token or filesystem error string crosses
IPC. Stable categories cover invalid_request, not_found, unauthorized, api,
network, filesystem, decode, operation_conflict and internal.

Error::Other is not a stable integrity taxonomy. Map it conservatively to internal;
never parse its text into invented codes. Identifying integrity failures precisely
would require a future typed base-library error contract.

## TypeScript behavior

The client offers typed queries, start/status, event subscriptions and a per-operation
watch helper. Every subscription returns explicit cleanup. Cleanup during asynchronous
listener setup must remove the listener once setup finishes. Listener errors are
reported, not silently swallowed. No React/Zustand/UI dependencies or product policy.

### Client API

The package exports `createClient`, `createTauriTransport`, their public interfaces,
and the generated contract types and identifiers. `createClient()` uses the real
Tauri adapter; `createClient(transport)` injects a transport for headless tests.

```ts
import { createClient } from '@hostmylauncher/tauri';

const client = createClient();
const accepted = await client.startOperation({
  instanceId: 'managed-instance',
  kind: 'sync',
  code: null,
});

const cleanup = client.watchOperation(
  accepted.operationId,
  snapshot => {
    // Render operation state; queued acceptance is not installation readiness.
    // Verification issues belong to a succeeded result, not to execution errors.
    console.log(snapshot.status);
  },
  error => {
    // Subscription/recovery errors are separate from backend operation failures.
    console.error(error);
  },
);

// Call on component/screen disposal. This never cancels the backend operation.
cleanup();
```

Query/start methods accept the generated request object and return promises.
Nullable wire inputs such as code/limit/offset are explicit nulls, not omitted.
`subscribeOperations(onSnapshot, onError)` watches the session-wide event stream;
`watchOperation(operationId, onSnapshot, onError)` watches one operation and recovers
its latest state after registering the listener. Both return cleanup synchronously.
The error handler is required. Operation failures are represented in snapshots;
subscription or recovery failures are delivered to the error handler.

## Deferred to v2

The user explicitly deferred these capabilities until the public base-library API
supports them first:

- [ ] Detailed verification progress.
- [ ] Typed integrity errors, without parsing generic error messages.
- [ ] Cancellation/interruption of operations, including defined cleanup and
      filesystem consistency guarantees.

The v1 bridge uses indeterminate verification progress, conservative safe generic
errors, and no cancellation. Do not emulate these missing capabilities in the
adapter. This is a roadmap, not a v1 implementation requirement.

## Verification

Use isolated temporary installations and a local mock provider. Exercise rejection
without side effects, state recovery/correlation, same-resource conflicts, safe
errors/results, base sync/verify/repair behavior, cleanup and stale-event handling.
The minimal example consumes both packages. Mocked IPC contract tests are not proof
of execution inside a real Tauri WebView; document that distinction.

The README's future CI checklist remains pending until enforced in CI. Local
checks must cover generated DTOs and actual command/event registrations, not just
matching payload shapes.
