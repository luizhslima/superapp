# 07 · Qualidade, CI e Release

## Estratégia de testes

| Nível | Ferramentas | Escopo |
|---|---|---|
| Unidade Rust | `cargo test`, `proptest` | Core, validação de payload, capabilities |
| Unidade TS | Vitest | Shell, SDK |
| Integração | Rust + Wasmtime | Mini-apps, fuel/memória, WIT |
| E2E desktop | `tauri-driver` / WebDriver | Fluxos nos 3 SOs |
| Segurança | Suíte de ataques, `cargo-audit`, `cargo-deny`, fuzz (`cargo-fuzz`) em parsers de manifesto/IPC | Gate Fase 2 |
| Performance | Criterion + scripts de cold start/RAM | Metas de [00](00-vision-scope.md) |

## CI (GitHub Actions)
- Matriz: `windows-latest`, `macos-latest`, `ubuntu-latest`.
- Jobs: lint (clippy, rustfmt, eslint) → build → testes → E2E → benchmarks (nightly ou por tag).
- Cache: `sccache`/`Swatinem/rust-cache`, store do pnpm.
- Execução condicional por caminho (ex.: não rebuildar Python/C++ se não mudou).

## Release
- Assinatura: Windows (certificado de code signing), macOS (Developer ID + notarização), Linux (AppImage/deb com assinatura).
- Auto-update do Tauri com chave de assinatura própria.
- Versionamento semântico; changelog automatizado; tags disparam release.
- Configurar cedo (risco R10), mesmo que com artefatos não públicos.

## Observabilidade
Logs estruturados (`tracing`); log de auditoria de segurança; telemetria opt-in e anônima (Fase 5); relatório de crash local.

## Definição de pronto (DoD)
Código + testes; CI verde nos 3 SOs; sem regressão de benchmark acima do limite; ADR/doc atualizado quando contrato muda.
