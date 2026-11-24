# Rust bridge development

This checkout uses the read-only sibling `../../../hostmylauncher-rs` as a local
Cargo dependency. Publication remains disabled. Rust transport DTOs use ts-rs
derives; generated TypeScript is not a handwritten copy.

## Service and host wiring

`Bridge::new(client, catalog, locator, sink)` creates a cloneable session
service. Its six async methods accept the generated request DTOs. The host supplies
its `hostmylauncher::Client`, allowed launcher IDs, and an `InstanceLocator`: any
`Fn(&str, &str) -> Result<InstanceLocation, LocateError>` that answers from the
launcher's own catalog of installations. `LocateError::NotInstalled` rejects a start
with `not_found`, `LocateError::Busy` with `operation_conflict`. IPC never configures
these resources or the bearer token. Keep the service alive for the backend session;
UI cleanup never cancels accepted work. Event sinks receive complete snapshots
outside service locks; their panics are caught. Querying state remains authoritative
when event delivery fails.

Located paths must be absolute host paths without parent traversal components. IDs are
nonempty ASCII alphanumeric/dot/underscore/hyphen, excluding `.` and `..`; access
codes cannot be blank. Existing directory aliases are canonicalized to their actual
roots, including aliases in ancestors of missing paths. Manifest symlinks are
rejected. Admission locates and resolves resources on every start and checks overlapping roots, manifests,
and root/manifest intersections before allocating an operation ID, issuing HTTP,
or modifying files. This does not protect against another process changing paths
or installations while work is active.

The locator runs under the admission lock, so it must not call into the bridge.
To keep its own work off an installation, a host records a reservation where the
locator sees it (the locator then answers `Busy`), releases its own locks, and then
checks `Bridge::is_busy(launcher, instance)`. One of the two always sees the other.
`has_active_operations()` covers exit protection.

The default Cargo feature set has no Tauri dependency. Enable `plugin` for a real
Tauri 2 plugin; Tauri default features remain disabled and the application supplies
its own runtime. Exact Rust wiring:

```rust,ignore
use std::sync::Arc;
use hostmylauncher_tauri::{Bridge, InstanceLocation, LocateError};

let service = Bridge::new(
    client,                       // Host-configured hostmylauncher::Client
    vec![launcher_id],            // Launcher IDs the app may query
    move |launcher: &str, instance: &str| {
        // Answer from the launcher's own catalog of installations.
        match installations.lookup(launcher, instance) {
            Some(found) if found.in_use => Err(LocateError::Busy),
            Some(found) => Ok(InstanceLocation {
                destination: found.path.clone(),             // Absolute host-owned PathBuf
                manifest_path: found.path.join(".hostmylauncher-manifest.json"),
            }),
            None => Err(LocateError::NotInstalled),
        }
    },
    Arc::new(|snapshot| {
        // Optional host observer; no event subscription required to retain state.
        let _ = snapshot;
    }),
)?;

let builder = tauri::Builder::default()
    .plugin(hostmylauncher_tauri::plugin::init(service));
// Continue the application's own setup and run with its generated context.
```

Plugin setup manages the service and attaches an app emitter automatically, using
`hostmylauncher://operation`. Do not separately manage a different Bridge. The
plugin initializer is generic over the host Runtime. The shared command macro in
`src/commands.rs` drives the actual `tauri::generate_handler!`, generated identifiers,
and build-time permission metadata. The handler declares its plugin identity so
Tauri's unused-command filtering retains the correctly namespaced commands.

The consuming Tauri application's capability must allow `hostmylauncher:default`
for the intended local windows, or select individual permissions:

- `hostmylauncher:allow-list-instances`
- `hostmylauncher:allow-get-instance`
- `hostmylauncher:allow-resolve-by-code`
- `hostmylauncher:allow-get-instance-files`
- `hostmylauncher:allow-start-operation`
- `hostmylauncher:allow-get-operation`

The consumer also configures the normal Tauri event-listen/unlisten capabilities
used by its client. The plugin does not automatically grant capabilities or expose
commands to remote content. Build metadata follows Tauri's permission-files
convention using Cargo `links = "hostmylauncher"`; generated permission files stay
in OUT_DIR. Mock IPC tests explicitly allow local commands in their test context;
this is not an ACL bypass installed by production plugin setup.

## Parent-owned verification

Run from the repository root using the parent's background execution tool. Ensure
`target/test-tmp` exists on disk-backed storage first. Parent observed the staged
public API RED (exit 101: absent Bridge and InstanceRegistration) before behavior
implementation. Parent also successfully generated the TypeScript module.
Current observed check evidence is recorded in ../../docs/verification.md.
Service GREEN and plugin compilation/MockRuntime execution are now established.

Focused service checks:

```sh
TMPDIR="$PWD/target/test-tmp" cargo test -p hostmylauncher-tauri --test public_bridge
TMPDIR="$PWD/target/test-tmp" cargo test -p hostmylauncher-tauri --test service_regressions
```

Generation and consistency:

```sh
TMPDIR="$PWD/target/test-tmp" cargo run -p hostmylauncher-tauri --bin generate-contract
TMPDIR="$PWD/target/test-tmp" cargo run -p hostmylauncher-tauri --bin generate-contract -- --check
TMPDIR="$PWD/target/test-tmp" cargo test -p hostmylauncher-tauri --test contract_generation
```

Full Rust and plugin checks:

```sh
cargo fmt -p hostmylauncher-tauri -- --check
TMPDIR="$PWD/target/test-tmp" cargo test --workspace
TMPDIR="$PWD/target/test-tmp" cargo check -p hostmylauncher-tauri --features plugin
TMPDIR="$PWD/target/test-tmp" cargo test -p hostmylauncher-tauri --features plugin-tests --test plugin_ipc
```

`plugin-tests` enables `plugin` plus Tauri's test utilities, without Wry. The tests
invoke the real six handlers on MockRuntime and observe actual Tauri emit payloads.
This is not proof of execution inside a real WebView. Consuming-app capability
discovery is tested separately by examples/capability-host, without inline grants. Tauri may still require native platform development libraries even
with Wry disabled; report environmental build failures separately.

## Semantics and limitations

Accepted starts return queued snapshots. Session work progresses to running and
then succeeded/failed, with monotonically increasing per-operation sequences.
Success is recorded only after the base returns, including manifest persistence.
Late progress cannot alter terminal snapshots. Sink or task panics become safe
internal failures and release resource guards; terminal event sink failure does
not replace the authoritative outcome.

Sync, verify, and repair summaries project actual base reports. Verification
findings are successful results. Unchanged sync preserves local modifications;
repair intentionally restores provider content. Verification is indeterminate;
repair exposes only actual download callbacks. Download counters may reset between
files or attempts; zero totals become null. Install counts are preparation entries,
not committed replacements. Extract units are archives. The base currently does
not emit Remove. No overall percentage, cancellation, invented detailed verification
progress, or typed generic integrity taxonomy is provided. Error::Other is mapped
to internal without parsing. Only typed HTTP status context is public. No error
message guarantees that every failed operation rolled back successfully.

## Temporary resources and cleanup

Every fixture is a scoped `tempfile::TempDir`; only small artifacts and JSON
manifests are used. Mock providers listen locally, never at production endpoints.
`TMPDIR` routes both fixtures and base verification staging to disk-backed project
storage rather than `/tmp` tmpfs. RAII cleans fixture and staging directories when
tests finish normally, including assertion unwinding.

If the runner is forcibly terminated, cleanup may not run. Inspect only test-created
artifacts under `target/test-tmp` and authorize any residual cleanup explicitly.
Never remove unrelated `/tmp` contents or all project build artifacts. Cargo build
outputs and generated contracts/permission metadata are intentional outputs.

## Key Learnings

- Base remote file `path` is the complete relative filename; `name` is not appended.
- Verification of unhashed files may download a reference into temporary storage,
  but still exposes no detailed verification progress.
- Real Tauri plugin dispatch checks capabilities even on MockRuntime. Command
  metadata matching alone does not prove command invocation.
- The base publicly re-exports progress/report types; its download module is private.
- Secret-bearing request DTOs omit Debug, and snapshots never retain request codes.
