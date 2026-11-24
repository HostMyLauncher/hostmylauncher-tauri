# Minimal headless consumer

This example consumes the real Rust Bridge service and the built public TypeScript
client through a tiny JSON-lines subprocess transport, with a local mock provider.
It verifies queued acceptance, recoverable operation state and successful
verification reporting a missing file without creating the installation.

This is **not** a Tauri WebView application, nor proof of permissions or command
execution inside a real WebView. Real Tauri registration is tested separately by
the bridge's plugin-feature tests. The example transport supports only the three
commands needed by this smoke test; it is not a production IPC replacement.

## Run

From the repository root:

```sh
mkdir -p target/test-tmp
export TMPDIR="$PWD/target/test-tmp"
export CARGO_TARGET_DIR="$PWD/target"
pnpm build
cargo build --manifest-path examples/minimal-launcher/backend/Cargo.toml
node --test examples/minimal-launcher/smoke.test.mjs
```

Cargo may need to fetch dependencies. The Node runner requires Node 22 or newer.
The example is a separate, unpublished Cargo workspace that uses both packages
from this repository. It has no frontend framework or production provider credentials.

Fixtures are small scoped TempDir directories under TMPDIR and are cleaned when
the backend exits. Cargo's target output is intentional build output, not fixture
garbage. The launcher remains responsible for Minecraft and full-installation state.
