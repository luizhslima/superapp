# ADRs (Architecture Decision Records)

Formato: Status · Contexto · Decisão · Consequências · Alternativas · Riscos. Mudança de arquitetura = novo ADR (ou atualização de status com data).

| ADR | Título | Status |
|---|---|---|
| [001](ADR-001-tauri-v2.md) | Tauri v2 como host | Accepted |
| [002](ADR-002-rust-orchestrator.md) | Rust como orquestrador | Accepted |
| [003](ADR-003-python-embedded-vs-sidecar.md) | Python embutido vs sidecar | Accepted (S1: worker externo) |
| [004](ADR-004-wasm-miniapps.md) | Wasm/Wasmtime para mini-apps | Accepted (WASI 0.2, S2) |
| [005](ADR-005-binary-ipc-arrow.md) | Binary IPC + Arrow | Accepted (S3 Windows; Arrow IPC a medir) |
| [006](ADR-006-capabilities-isolation.md) | Capabilities + Isolation Pattern | Accepted |
| [007](ADR-007-cpp-optional-cxx.md) | C++ opcional via cxx | Proposed (adiado; S4 só se houver caso de uso) |
| [008](ADR-008-monorepo-tooling.md) | Monorepo e tooling | Accepted (`cargo xtask`) |
