# Session provider service

`Bridge::new(client, catalog, registrations, sink)` creates a cloneable Rust service
independent of a Tauri runtime. Host-owned instance registrations supply launcher
ID, instance ID and absolute destination/manifest paths. No IPC request supplies
disk paths, remote URLs, client configuration or bearer credentials.

The service exposes typed provider queries plus distinct sync, verify and repair
operations. `start_operation` returns queued acceptance, not completed installation.
`get_operation` recovers the authoritative session snapshot, including operation ID,
instance ID, kind, sequence, progress and terminal result/error. State is not persisted
across process exit. The launcher owns full-installation readiness and Minecraft.

Admission validates registered resources and rejects active overlapping canonical
roots or manifest resources before allocating an ID, issuing HTTP or touching files.
This is an in-process guard, not a cross-process lock or crash-atomic guarantee.
Host callbacks execute outside service locks; panics do not strand active guards.
Terminal snapshots are not revived by late progress callbacks.

Verification differences are successful results. Sync preserves local edits when
provider content is unchanged; explicit repair restores missing/different expected
files. Counters come from base reports. Download progress is per file/attempt and
may reset; preparation counts are not committed installations. Verification is
indeterminate. Completion follows the base return, including manifest persistence.

Errors use safe broad typed codes; raw upstream messages, URLs and request codes
are never copied into public snapshots. Generic errors do not promise successful
rollback. Detailed verification progress, typed integrity errors and cancellation
are deferred until the base public API supports them.

```sh
mkdir -p target/test-tmp
TMPDIR="$PWD/target/test-tmp" cargo test -p hostmylauncher-tauri --test public_bridge --offline
TMPDIR="$PWD/target/test-tmp" cargo test -p hostmylauncher-tauri --test service_regressions --offline
```

Tests use small scoped fixtures and a local mock provider. Use package-scoped
formatting and keep the sibling base read-only. Runtime Tauri bindings are a separate
unit; adding this service alone does not configure a consuming application's ACL.
