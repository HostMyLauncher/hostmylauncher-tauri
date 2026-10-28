# V1 verification status

## Observed implementation checks

| Check | Result |
| --- | --- |
| Rust public-interface RED | Exit 101: Bridge and InstanceRegistration absent before implementation |
| Rust focused GREEN | 8 tests passed |
| Rust workspace suite | 15 tests passed: public 8, service regressions 5, contract 2 |
| Generated TypeScript consistency | generate-contract --check passed |
| Tauri plugin compilation | Passed |
| Actual MockRuntime IPC | 3 tests passed, all six bindings and event exercised |
| TypeScript typecheck/build | Passed |
| TypeScript public client/adapter tests | 16 tests passed after initial missing-client RED |
| Real Rust service + built TS headless consumer | Build and smoke test passed after missing-example RED |

Checks use small scoped fixtures and project-confined disk-backed TMPDIR. MockRuntime
and the headless subprocess example do not establish real WebView execution; the
separate native Wry result below is the real-WebView evidence.

## Real WebView fixture status

`examples/webview-validation` contains a separate Tauri 2/Wry app and UI that use
the real built TypeScript client, a local-only mock provider, and a scoped `TempDir`
destination. In the observed run, the UI received download progress and terminal
events, recovered terminal state through `getOperation`, and called subscription
cleanup. The host verified the downloaded fixture file and wrote the proof JSON
before exiting. Source and compilation alone are not real WebView proof; the
successful native run recorded below is.

The fixture-only Cargo lock was aligned to Tauri 2.12.4 after the initial lock
resolved incompatible `tauri-runtime` and `tauri-runtime-wry` versions. The final
offline `cargo check --manifest-path examples/webview-validation/Cargo.toml` passed.
An earlier Vite build attempt timed out after 30 seconds; after the user explicitly
authorized an unbounded rerun, the Vite build and native Wry app run both completed
successfully. The app launched in the graphical session and exited 0. Its result
JSON, `target/test-tmp/webview-proof-1793157044804709258.json`, records
`webview: true`, `downloadProgressEvent: true`, `terminalEvent: "succeeded"`,
`recovered: "succeeded"`, `subscriptionCleanupCalled: true`,
`destinationVerified: true`, and one downloaded mock file. The provider was local
wiremock only; the destination and manifest were inside a scoped `TempDir`. This
proves the fixture's actual WebView IPC path, not integration with a downstream
launcher or use of real launcher data. GTK/WebKitGTK and an interactive display were present; no
missing system library was identified.

## Independent verification limits

The independent read-only source review found no concrete source blocker. Its
TypeScript checks and headless smoke test also passed. The initial independent
Rust attempt used an incomplete external scratch and established no proof. After
user authorization to close the gaps, a generic read-only verifier ran the cached
checks directly offline: workspace 15 and plugin 3 tests passed, generator --check
and package-scoped formatting passed, all exit 0. No workspace/cache copies or
installs were performed. Base Git status remained clean and target/test-tmp had
zero residual entries. The independent Rust execution gap is now closed.

The consuming application's discovery and resolution of `hostmylauncher:default`
is now proven by examples/capability-host: its actual tauri-build invocation failed
without the bridge dependency (permission not found), then succeeded with the real
plugin dependency. The generated Context<MockRuntime> also compiled. An intervening
missing fixture icon was corrected with a minimal RGBA PNG, not a bridge change.
No manual ACL or inline permission injection was used. This is build/discovery
proof, distinct from MockRuntime dispatch and real WebView execution.

Native assessment initially failed on untracked scope; staging the intended files
resolved it. The full-candidate START then exceeded the lens context budget before
creating authority. Explicit local-commit authorization enabled coherent smaller
candidates. The initial reviewer-model configuration blocker was resolved with the
user-authorized project-local model route, not a global configuration change.

## Reviewed implementation units

Each range contains exactly one work-unit commit. Every review below reached
native approved closure and its exact acknowledgement consumed the authority.
No STATUS was issued after a successful burn.

| Commit | Unit | Native lineage | Lenses |
| --- | --- | --- | --- |
| c9e41e0 | Generated Rust/TS transport contract | review-03eb3201bb476b71 | risk, resilience, readability, reliability |
| b5acef9 | Rust session service | review-d01ce11e2681cae8 | risk, resilience, readability, reliability |
| 5599fc1 | Tauri plugin and IPC tests | review-1341b0999dee5546 | risk, resilience, readability, reliability |
| aaf9382 | TypeScript client and IPC documentation | review-4cc0bdb5519a7f16 | resilience, readability, reliability |
| d1ebbbc | Headless consumer | review-4083b1a349f3f10c | resilience, readability, reliability |
| 64b8ec6 | Consuming-host capability fixture | review-542e90c508eab6b1 | risk, readability, reliability |

The final verification-documentation snapshot is a separate passive work unit;
its own review result belongs to native authority and session memory, not a
self-referential claim in the candidate being reviewed.

Non-blocking native advisories remain separate follow-ups, not corrections or
reasons to reopen these approved candidates:

- Contract: R3-wire-serialization-coverage (contract_generation.rs:28-34).
- Service: R3-runtime-shutdown (bridge.rs:350-423).
- Plugin: R3-event-wait (plugin_ipc.rs:196-197).
- Client: R4-terminal-recovery (client.ts:78-82).
- Headless consumer: R3-noncreation-unproved (smoke.test.mjs:9-17); the smoke
  does not independently assert installation-directory noncreation.

## Fresh resume checks

All executed offline or from installed dependencies, with project-confined TMPDIR:

- Plugin IPC: 3 passed; package-scoped formatting passed.
- TypeScript: typecheck, 16 tests and build passed.
- Headless Rust consumer: build and Node smoke passed (1 test).
- Capability host: actual consuming-app cargo check passed.
- Rust workspace: 15 passed; generated contract --check current.

Independent execution, consuming-host build, and local real-WebView proofs are
closed for their respective fixtures. The WebView proof is limited to the mock
provider and isolated destination described above; CI remains a future task, and
reproducible published dependency sourcing/license selection remain prerequisites
for publication. No push, pull request or publication occurred.

## Resolved scope/storage incident

Workspace-wide Cargo formatting traversed the local base dependency. Four
formatting-only edits were restored with user authorization after checking each
file against the exact formatted baseline; the base Git working tree is clean.
Use `cargo fmt -p hostmylauncher-tauri -- --check`, not `cargo fmt --all`.

The exact verifier scratch directory was removed with user authorization. Its
pnpm store was inspected and preserved because ownership of individual cache
entries could not be established. No unrelated temporary files were deleted.
