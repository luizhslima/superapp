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

## Métricas-alvo

Revisadas no fechamento da Fase 1 (2026-11-10) com medições reais (Windows local; Linux/macOS só em runners de CI). Os alvos originais da investigação ficam na coluna da esquerda para rastreabilidade.

| Métrica | Alvo original | Medido na Fase 1 | Alvo revisado |
|---|---|---|---|
| Cold start (UI pronta) | 0,3–0,8 s | Windows 0,5–4 s (varia com cache do WebView2); macOS CI ~5,4 s; Linux CI ~32 s (xvfb, sem GPU) | **≤ 1,5 s com WebView2 aquecido** em máquina física. Valores de CI são só informativos (aviso > 15 s). Medir cold real em máquina de referência |
| RAM ociosa | 25–40 MB | Host Rust **25–30 MB** (Windows). Soma de RSS do WebView2 ~350 MB (superestima: páginas compartilhadas contadas várias vezes) | **Host ≤ 40 MB**. WebView medido **separadamente** (working set privado, máquina física); sem alvo até haver medição confiável. A meta original só vale para o host |
| Instalador base | ~4–5 MB | Antes do Wasmtime: NSIS 1,9 MB. **Com Wasmtime + Cranelift (Fase 2): NSIS 5,55 MiB, binário 24,1 MB** (28,7 MB sem LTO/strip/features cortadas). dmg/deb/AppImage ainda a remedir | **≤ 6 MiB com o runtime Wasm** (NSIS/dmg/deb), aceito por decisão em 2026-11-10. O Cranelift responde por ~15 MB do binário; reduzir mais exige runtime só-AOT (ver TODO). AppImage fora da meta por construção |
| Cold start de mini-app Wasm | < 1 ms (AOT) | ~36 µs (S2, `InstancePre`) | Mantido: < 1 ms |
| Memória por mini-app | 1–10 MB | Não medido (Fase 2) | Mantido, com limite configurável por manifesto |
| Binary IPC | 5–10× vs JSON em payloads grandes | Host→UI 10 MiB em ~14–120 ms conforme SO (teto ~110 MiB/s é do WebView2/Windows). **UI→host com Isolation ~1 MiB/s** | Mantido para host→UI. Para UI→host: `ipc_bytes` ≤ 1 MiB; acima disso, `sapp://` (~65 MiB/s no Windows, ~85× mais rápido). Ver ADR-005/006 |

## Critério de sucesso do MVP

Um host assinado e empacotado nos 3 SOs, com shell multi-janela, IPC binário, capabilities, e ao menos um mini-app Wasm de terceiros-simulado executando dentro dos limites, mais um malicioso contido pela suíte de ataques.
