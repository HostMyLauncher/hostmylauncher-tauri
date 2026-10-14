# Transport contract generation

Rust DTOs in `contract.rs` are the source of truth for TypeScript transport types.
A shared Rust command list supplies generated command identifiers and opt-in
Tauri permission metadata; the event identifier is also generated. Runtime
service/handler integration is a separate unit from transport generation.

```sh
mkdir -p target/test-tmp
TMPDIR="$PWD/target/test-tmp" cargo run -p hostmylauncher-tauri --bin generate-contract
TMPDIR="$PWD/target/test-tmp" cargo run -p hostmylauncher-tauri --bin generate-contract -- --check
TMPDIR="$PWD/target/test-tmp" cargo test -p hostmylauncher-tauri --test contract_generation
```

The `--check` command never rewrites a stale/missing generated file. Regenerate
explicitly when transport types or identifiers change. Generated TypeScript is
not edited by hand. JSON fields use camelCase, enum values snake_case and nullable
fields explicit nulls. Secret-bearing request DTOs intentionally omit Debug.

The base dependency currently points to the sibling development checkout. All
packages remain non-publishable. Use package-scoped formatting; do not format the
read-only sibling base through `cargo fmt --all`.
