# Plano de Desenvolvimento — Superapp Desktop Híbrido

Transforma a [investigação v1](../investigation-v1.md) em um plano executável para **dev solo + IA**, alvo **Windows, macOS e Linux**.

**Status**: rascunho v0.1 · fase atual: pré-Fase 0

## Leitura recomendada (núcleo)
1. [02 · Roadmap por fases](02-roadmap-phases.md)
2. [03 · Riscos e spikes](03-risks-and-spikes.md)
3. [01 · Arquitetura](01-architecture-overview.md)

## Índice
| Doc | Conteúdo |
|---|---|
| [00](00-vision-scope.md) | Visão, escopo, métricas-alvo |
| [01](01-architecture-overview.md) | Camadas, fronteiras de confiança, fluxos |
| [02](02-roadmap-phases.md) | Fases 0–5 com gates |
| [03](03-risks-and-spikes.md) | Riscos e spikes S1–S5 |
| [04](04-adrs/README.md) | Decisões de arquitetura (ADRs) |
| [05](05-backlog-epics.md) | Épicos e critérios de aceite |
| [06](06-security-model.md) | Threat model e ciclo de vida de mini-apps |
| [07](07-quality-ci-release.md) | Testes, CI, release |
| [08](08-repo-structure.md) | Layout do monorepo |
| [09](09-working-with-ai.md) | Fluxo dev solo + IA |
| [TODO](TODO.md) | Pendências vivas, validação macOS/Linux, custo zero |

## Síntese das principais conclusões
- A arquitetura da investigação é sólida, mas **otimista**: PyO3 multi-interpretador, WASI 0.3 e o custo de bundling do Python são os maiores riscos.
- Estratégia: **spikes primeiro**, depois fatias verticais: Host seguro → Mini-apps Wasm → Python → C++ (opcional).
- Python e C++ são **módulos opcionais** para preservar a promessa de leveza.

## Glossário
**IPC** comunicação entre processos/camadas · **WIT** linguagem de contratos do Component Model · **WASI** interface de sistema do Wasm · **Capability** permissão declarativa com escopo · **ADR** registro de decisão arquitetural · **Gate** critério de saída de fase.
