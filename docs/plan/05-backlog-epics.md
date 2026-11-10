# 05 · Backlog de Épicos

Cada épico mapeia para uma fase de [02-roadmap-phases.md](02-roadmap-phases.md). Critérios de aceite (CA) são verificáveis.

## Fase 0 — Fundação e Spikes

**E0.1 Monorepo e CI base** (ADR-008)
- H: estrutura de pastas, cargo workspace, pnpm workspace, `just`/xtask.
- H: CI com matriz win/mac/linux: build + lint + testes.
- CA: push em branch dispara CI verde nos 3 SOs.

**E0.2 Spikes S1–S5** ([03](03-risks-and-spikes.md))
- CA: cada spike tem resultado registrado em [03](03-risks-and-spikes.md) e ADR atualizado. *(S1, S2, S3, S5 feitos no Windows; S4 adiado.)*

## Fase 1 — Casca Segura

**E1.1 Core Rust e roteador IPC**
- H: registro de comandos tipados; logging estruturado; erros padronizados.
- CA: comando fora de escopo retorna erro e gera log de auditoria.

**E1.2 Capabilities e Isolation Pattern** (ADR-006)
- CA: UI sem permissão não lê FS; tentativa de path traversal bloqueada em teste.

**E1.3 Shell multi-janela e micro-frontends**
- H: shell, 2 micro-frontends de exemplo em WebviewWindow, BroadcastChannel.
- CA: travar um micro-frontend não congela o shell.

**E1.4 Binary IPC** (ADR-005)
- CA: payload de 10 MB trafega sem JSON; benchmark registrado.

**E1.5 Empacotamento e benchmarks**
- CA: instaladores por SO; relatório cold start/RAM/tamanho gerado no CI.

## Fase 2 — Mini-Apps Wasm

**E2.1 Host Wasmtime** — componentes carregados, fuel e limites de memória (ADR-004). WASI 0.2 + async no host (S2); **o loader deve recusar engine sem `consume_fuel`**; traduzir OOM do guest em erro legível.
**E2.2 WIT da API do host v0** — storage, rede com allowlist, UI/eventos, logging.
**E2.3 Pacote e manifesto** — formato, esquema versionado, assinatura, verificação.
**E2.4 Ciclo de vida e consentimento** — instalar/executar/suspender/remover; UI de permissões.
**E2.5 SDK e exemplos** — mini-apps em Rust e outra linguagem.
**E2.6 Suíte de ataques** — loop infinito, memória, FS/rede não declarados, manifesto adulterado.
- CA global: 100% dos ataques contidos; host permanece responsivo.

## Fase 3 — Python/IA

**E3.1 Runtime Python opcional** — download sob demanda, verificação de integridade (ADR-003). Avaliar `python-build-standalone`/embeddable para reduzir os ~133 MB.
**E3.2 Worker Python e ciclo de vida** — processo filho gerenciado pelo Rust (Job Object no Windows, `PR_SET_PDEATHSIG` no Linux); pool de N workers; sem zumbis; falha recuperável. *(Substitui o pool de subinterpretadores, refutado em S1.)*
**E3.3 Ponte Arrow IPC** — Rust↔worker por pipe/memória compartilhada; **primeiro medir latência/throughput** (pendência do S1).
**E3.4 Feature de IA/dados de prova** — exposta à UI e ao WIT via capability.
- CA: latência e throughput do Arrow IPC publicados; falha do worker não derruba o host.

## Fase 4 — C++ (sob demanda)

**E4.1 Bridge `cxx`/`cxx-async`** (ADR-007) — exceções→`Result`, async com Tokio.
**E4.2 Módulo real** (ex.: llama.cpp) — build reproduzível nos 3 SOs.

## Fase 5 — Ecossistema e Endurecimento

**E5.1 Distribuição de mini-apps assinados** · **E5.2 Auto-update seguro** · **E5.3 Telemetria opt-in** · **E5.4 Auditoria de segurança** · **E5.5 Documentação de desenvolvedor** · **E5.6 Beta fechado**.
