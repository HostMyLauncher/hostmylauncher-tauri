# HostMyLauncher Tauri bridge

Optional bridge scaffold with Cargo and pnpm workspaces. The base library remains
independent; launchers own orchestration, Minecraft and product behavior.
Publishing is disabled and no runtime IPC is implemented in this baseline.

## Future task: CI divergence checks

- [ ] Rust compilation and tests.
- [ ] Type regeneration with no pending differences.
- [ ] TypeScript typecheck and tests.
- [ ] Command/event name correspondence as well as payload compatibility.
- [ ] Integration of both packages in a minimal consumer.

Matching payloads alone does not prove that a command exists.
