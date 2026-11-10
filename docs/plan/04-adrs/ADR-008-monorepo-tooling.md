# ADR-008 · Monorepo e tooling

- **Status**: Accepted (2026-11-10) — orquestrador: `cargo xtask`
- **Contexto**: Múltiplos componentes (Rust, TS, WIT, Python, C++) mantidos por uma pessoa + IA.
- **Decisão**: Monorepo único; Cargo workspace para Rust; pnpm workspaces para UI/SDK TS; WIT em diretório próprio versionado; CI GitHub Actions com matriz Windows/macOS/Linux; `just` ou `cargo xtask` como orquestrador de tarefas.
- **Consequências**: (+) mudanças atômicas entre contratos (WIT/IPC) e consumidores. (−) CI mais lento; usar cache (sccache, pnpm store) e jobs por caminho alterado.
- **Detalhes**: [08-repo-structure.md](../08-repo-structure.md).
