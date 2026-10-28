# Real Tauri WebView proof (local mock only)

This is an independent, disposable Tauri 2/Wry application that runs the actual
`@hostmylauncher/tauri` client inside Tauri's real WebView. It queries one mock
instance, starts one `sync`, observes download progress and a terminal event,
recovers terminal state with `getOperation`, and cleans up its subscription. Rust serves
only a local `wiremock` provider and keeps destination/manifest paths inside a
`TempDir`; no production endpoint, real launcher data, or downstream launcher client is used.
The proof does not emulate v1 cancellation, detailed verification progress, or
typed integrity errors. The success/failure result is written to the path supplied in
`HOSTMYLAUNCHER_WEBVIEW_RESULT`, and the app exits when the WebView reports.

Unlike the headless minimal-launcher smoke, this exercises Tauri's Wry runtime,
real WebView JavaScript globals, plugin command invocation, capability discovery,
and event delivery. It is intended as a bounded integration proof, not a production launcher or
evidence for real installation data. A compile alone is not WebView proof: only a
successful app run (exit 0 and a result JSON with `webview`, `downloadProgressEvent`,
`terminalEvent`, `recovered`, `subscriptionCleanupCalled`, and `destinationVerified`)
counts.

## Run (Linux desktop session)

From repository root, with Node 22+, Rust and the platform WebKitGTK/Tauri build
prerequisites installed:

```sh
pnpm --filter @hostmylauncher/tauri build
cd examples/webview-validation/ui
pnpm install --frozen-lockfile
pnpm build
cd ../../..
mkdir -p target/test-tmp
HOSTMYLAUNCHER_WEBVIEW_RESULT="$PWD/target/test-tmp/webview-proof.json" \
  CARGO_TARGET_DIR="$PWD/target" TMPDIR="$PWD/target/test-tmp" \
  cargo run --manifest-path examples/webview-validation/Cargo.toml
```

The Vite dependency and lockfile are fixture-local; there are no root workspace
or global dependencies. Cargo may fetch the fixture's Rust dependencies. `target/test-tmp/webview-proof.json` contains `{ "ok": true, ... }`
on success; a failed WebView flow writes `{ "ok": false, ... }` and exits nonzero.
The Rust fixture uses a small isolated temporary directory which is dropped when
the process exits. Cargo target output and the explicit result file are retained
under the repository's `target/`.
