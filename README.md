# hostmylauncher-tauri

Tauri 2 bridge between [`hostmylauncher-rs`](https://github.com/HostMyLauncher/hostmylauncher-rs)
and a launcher's frontend: typed IPC commands, operation state and progress events
for HostMyLauncher instances.

The launcher keeps its own catalog of installations and decides where they live.
The bridge stores none of that: when the frontend starts an operation, it asks the
launcher where that installation is.

## Packages

| Path | Package | Responsibility |
| --- | --- | --- |
| `crates/hostmylauncher-tauri` | `hostmylauncher-tauri` | Rust service, Tauri plugin, IPC DTOs, errors and operation events |
| `packages/tauri` | `@hostmylauncher/tauri` | Framework-independent TypeScript client and subscriptions |
| `examples/minimal-launcher` | — | Headless consumer: real Rust service + built TS client over a local mock provider |
| `examples/webview-validation` | — | Tauri/Wry app that runs the client inside a real WebView against a local mock provider |
| `examples/capability-host` | — | Build-only Tauri app proving the plugin's permissions are discovered by a consumer |

## Responsibilities

- **hostmylauncher-rs**: API client and the instance file engine (sync, verify, repair).
  It does not depend on Tauri or on this bridge.
- **This bridge**: typed commands and events, per-operation state, and protection
  against overlapping operations on the same files.
- **The launcher**: its installations and their folders, Minecraft, Java, loaders,
  accounts, launching and uninstalling. A finished sync means the provider files are
  in place, not that the instance is ready to play.

The TypeScript client has no UI framework dependency (no React, Zustand, etc.).

## Host setup (Rust)

Add the crate with the `plugin` feature:

```toml
[dependencies]
hostmylauncher = { git = "https://github.com/HostMyLauncher/hostmylauncher-rs", rev = "<commit>" }
hostmylauncher-tauri = { git = "https://github.com/HostMyLauncher/hostmylauncher-tauri", rev = "<commit>", features = ["plugin"] }
```

Use the same `hostmylauncher` revision as the bridge, so both share one `Client` type.

Create the bridge once at startup and register the plugin:

```rust
use std::sync::Arc;
use hostmylauncher_tauri::{Bridge, InstanceLocation, LocateError};

let bridge = Bridge::new(
    hostmylauncher::Client::new(),
    vec!["my-launcher".into()],          // Launcher IDs the frontend may query
    move |launcher: &str, instance: &str| {
        // Runs on every operation start; answer from the launcher's own catalog.
        match installations.lookup(launcher, instance) {
            None => Err(LocateError::NotInstalled),   // -> not_found
            Some(found) if found.in_use => Err(LocateError::Busy), // -> operation_conflict
            Some(found) => Ok(InstanceLocation {
                destination: found.path.clone(),      // absolute path
                manifest_path: found.path.join(".hostmylauncher-manifest.json"),
            }),
        }
    },
    Arc::new(|_snapshot| {}),            // optional Rust-side observer
)?;

tauri::Builder::default().plugin(hostmylauncher_tauri::plugin::init(bridge))
```

No IPC command accepts a path, a URL, the API base or a token: only the host decides
where files are written. Located paths must be absolute and free of `..`; the manifest
cannot be a symlink. Keep the bridge alive for the whole session: closing a screen
or unsubscribing never cancels an accepted operation.

### Keeping launcher work and operations apart

The bridge refuses overlapping operations on the same folders, but it does not know
when the launcher launches the game or uninstalls. Use both sides:

1. Before launching or uninstalling, record the reservation where the locator sees it
   (so it answers `Busy`), release your own locks, then check
   `bridge.is_busy(launcher, instance)`. If it is busy, undo the reservation.
   One of the two always sees the other.
2. Before exiting, check `bridge.has_active_operations()`.

The locator runs under the bridge's admission lock, so it must not call the bridge.
None of this protects against another process changing the files.

### Capabilities

Allow `hostmylauncher:default` in the app's capability for its local windows, or pick
individual permissions: `hostmylauncher:allow-list-instances`, `-get-instance`,
`-resolve-by-code`, `-get-instance-files`, `-start-operation`, `-get-operation`.
The client also needs the standard event `listen`/`unlisten` permissions. The plugin
never grants capabilities by itself.

## Frontend usage (TypeScript)

```ts
import { createClient } from '@hostmylauncher/tauri';

const client = createClient();
const accepted = await client.startOperation({
  launcherId: 'my-launcher',
  instanceId: 'some-instance',
  kind: 'sync',            // 'sync' | 'verify' | 'repair'
  code: null,              // access code for hidden instances
});

const cleanup = client.watchOperation(
  accepted.operationId,
  (snapshot) => {
    // queued -> running -> succeeded | failed
  },
  (error) => console.error(error), // subscription errors, not operation failures
);
// Call cleanup() when the screen goes away; the operation keeps running.
```

Nullable inputs are explicit `null`s. `watchOperation` registers its listener first and
then recovers the latest state, so no update is lost. Operation failures arrive as
`failed` snapshots with a typed error code; they never carry raw messages, URLs,
codes or tokens.

## Operations

| Kind | Effect |
| --- | --- |
| `sync` | Installs provider changes; keeps local edits to files the provider did not change. |
| `verify` | Reports missing or different files; changes nothing. Differences are a successful result. |
| `repair` | Restores missing or different files, including user edits. Keeps unrelated files. |

hostmylauncher-rs downloads several files at once. `download` progress counts the
bytes downloaded across all of the operation's files over their total size (null
when a file has no declared size); `install` progress counts prepared entries out
of the total. Verification has no detailed progress. Cancellation is not supported.

The full IPC contract (commands, payloads, progress, results and errors) is in
[docs/ipc-v1.md](docs/ipc-v1.md).

## Development

Requires Rust, Node 22+ and pnpm. Keep temporary test files off tmpfs:

```sh
mkdir -p target/test-tmp
export TMPDIR="$PWD/target/test-tmp"
pnpm install
cargo fmt -p hostmylauncher-tauri -- --check
cargo test --workspace
cargo test -p hostmylauncher-tauri --features plugin-tests --test plugin_ipc
cargo run -p hostmylauncher-tauri --bin generate-contract -- --check
pnpm typecheck && pnpm test && pnpm build
```

`plugin-tests` runs the six real command handlers on Tauri's MockRuntime, without a
WebView. Each example's README explains how to run it.

The Rust DTOs in `crates/hostmylauncher-tauri/src/contract.rs` are the source of truth
for the TypeScript types and the command/event names. After changing them, regenerate
`packages/tauri/src/generated/contract.ts` (never edit it by hand):

```sh
cargo run -p hostmylauncher-tauri --bin generate-contract
```

To work against a local hostmylauncher-rs checkout, override the git dependency in a
local, uncommitted `.cargo/config.toml`:

```toml
[patch."https://github.com/HostMyLauncher/hostmylauncher-rs"]
hostmylauncher = { path = "../hostmylauncher-rs" }
```

## Status

- Not published to crates.io or npm; no license chosen yet.
- No CI yet. It should run the commands above, including the contract `--check`, so
  the Rust commands and the TypeScript client cannot drift apart.
- Planned once hostmylauncher-rs supports them: detailed verification progress,
  typed integrity errors, and cancellation.
