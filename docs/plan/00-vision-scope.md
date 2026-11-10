# 00 · Visão e Escopo

> Fonte: [investigation-v1.md](../investigation-v1.md). Números de performance abaixo vêm da investigação e são **hipóteses a validar** (ver [03-risks-and-spikes.md](03-risks-and-spikes.md)).

## Visão

Um **superapp desktop híbrido** (Windows, macOS, Linux) que funciona como plataforma host: um núcleo leve e seguro em Rust que hospeda módulos de UI (micro-frontends), computação de IA/dados (Python embutido), código nativo de alto desempenho (C++) e **mini-apps de terceiros isolados em WebAssembly**.

## Princípios

1. **Segurança por padrão (Zero Trust)**: a UI e os mini-apps começam com zero privilégio; tudo passa por capabilities declarativas validadas pelo core Rust.
2. **Risco primeiro**: validar com spikes as apostas técnicas incertas antes de construir sobre elas.
3. **Fatias verticais finas**: cada fase entrega algo executável de ponta a ponta; uma camada de complexidade por vez.
4. **Core pequeno, módulos opcionais**: Python e C++ são opcionais/sob demanda para preservar tamanho e tempo de arranque.
5. **Contratos explícitos**: IPC tipado, WIT para mini-apps, manifestos de permissão versionados.
6. **Medir, não supor**: todo número-alvo tem benchmark automatizado.

## Não-objetivos (MVP)

- Mobile (iOS/Android).
- Marketplace público de mini-apps (apenas carregamento local/assinado até a Fase 5).
- Sincronização/nuvem, contas de usuário, monetização.
- Suporte a mini-apps em JS/TS puro fora do Wasm (avaliar depois via componentes JS compilados para Wasm).

## Personas

| Persona | Necessidade |
|---|---|
| **Usuário final** | App rápido, leve, seguro; instala/remove mini-apps com permissões claras. |
| **Dev de mini-app** | SDK/WIT estável, poliglota (Rust/C/C++/Go), ciclo de dev rápido, manifesto de permissões simples. |
| **Dev do host (você + IA)** | Monorepo previsível, CI nos 3 SOs, contratos versionados, testes de segurança. |

## Métricas-alvo (a validar)

| Métrica | Alvo (investigação) | Observação |
|---|---|---|
| Cold start | 0,3–0,8 s | Medir em cada SO, máquina de referência fixa |
| RAM ociosa | 25–40 MB | Sem Python carregado |
| Instalador base | ~4–5 MB | **Sem** CPython; Python como módulo opcional |
| Cold start de mini-app Wasm | < 1 ms (AOT) | Medir instanciação, não compilação |
| Memória por mini-app | 1–10 MB | Com limites configuráveis |
| Binary IPC vs JSON | 5–10× em payloads grandes | Benchmark com 100k triângulos / 10 MB |

## Critério de sucesso do MVP

Um host assinado e empacotado nos 3 SOs, com shell multi-janela, IPC binário, capabilities, e ao menos um mini-app Wasm de terceiros-simulado executando dentro dos limites, mais um malicioso contido pela suíte de ataques.
