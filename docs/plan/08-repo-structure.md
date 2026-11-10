# 08 · Estrutura do Repositório (proposta)

Ver [ADR-008](04-adrs/ADR-008-monorepo-tooling.md).

```
superapp/
├─ docs/
│  ├─ investigation-v1.md
│  └─ plan/                  # esta documentação
├─ src-tauri/                # app host Tauri (bin)
├─ crates/
│  ├─ core/                  # IPC, capabilities, ciclo de vida
│  ├─ miniapp-host/          # Wasmtime, loader, limites
│  ├─ py-bridge/             # gestor do worker Python + Arrow IPC (Fase 3, opcional)
│  └─ native-bridge/         # cxx/cxx-async (Fase 4, opcional)
├─ native/                   # código C++ + CMake (Fase 4)
├─ python/                   # pacotes Python do host (Fase 3)
├─ wit/                      # contratos WIT versionados da API do host
├─ ui/
│  ├─ shell/                 # shell e roteamento de janelas
│  └─ modules/               # micro-frontends
├─ sdk/                      # SDK para devs de mini-apps (Rust, TS tooling)
├─ miniapps/                 # exemplos e suíte de ataques
├─ spikes/                   # experimentos descartáveis (S1–S5)
├─ tools/                    # xtask/just, scripts de benchmark
└─ .github/workflows/
```

## Convenções
- Contratos (WIT, IPC, manifesto) versionados e alterados junto com consumidores no mesmo PR.
- `spikes/` é descartável: o resultado vive em ADR/relatório, não em código reaproveitado às cegas.
- Dependências opcionais (Python/C++) atrás de *feature flags* do Cargo.
- Um crate = uma responsabilidade; `core` não depende de `py-bridge`/`native-bridge`.
