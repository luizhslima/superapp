# 03 · Riscos e Spikes

A investigação é otimista. Este documento lista os riscos reais e os experimentos (spikes) que os resolvem **antes** de construir. Timebox pensado para dev solo + IA.

## Registro de riscos

| ID | Risco | Prob. | Impacto | Mitigação | Fallback | Spike/ADR |
|---|---|---|---|---|---|---|
| R1 | PyO3 não suporta bem subinterpretadores; extensões C (NumPy) incompatíveis com per-interpreter GIL | Alta | Alto | Spike S1 | Python 3.13 free-threaded, ou worker Python em processo separado com Arrow IPC | S1, ADR-003 |
| R2 | Distribuir CPython embutido nos 3 SOs (bundling, assinatura, tamanho) | Alta | Médio | Python como módulo opcional baixado sob demanda | Exigir Python do sistema (não recomendado) | S1, ADR-003 |
| R3 | WASI 0.3 / Component Model async imaturo no Wasmtime | Média | Alto | Spike S2 verifica versão estável e exemplos reais | WASI 0.2 + polling/host-async | S2, ADR-004 |
| R4 | `cxx`/`cxx-async` + CMake/toolchains em MSVC/Clang/GCC; CI caro | Média | Médio | Spike S4; adiar C++ até haver demanda | Módulo C++ em processo separado ou binding via Wasm | S4, ADR-007 |
| R5 | Isolation Pattern + Binary IPC + multi-webview incompatíveis ou lentos | Média | Alto | Spike S3 com benchmark | Desligar isolation só para canais binários com validação no Rust | S3, ADR-005/006 |
| R6 | Diferenças entre WebView2/WKWebView/WebKitGTK (CSP, WebGL, APIs) | Alta | Médio | Matriz de testes E2E nos 3 SOs desde a Fase 1 | Feature-detection e polyfills; reduzir superfície de APIs web | ADR-001 |
| R7 | Modelo de segurança de mini-apps falho (MiniCPRF, segredos embutidos, supply chain) | Média | Alto | Manifesto de permissões, assinatura, suíte de ataques | Revisão manual / allowlist | [06-security-model.md](06-security-model.md) |
| R8 | Escopo excessivo para dev solo (5 runtimes) | Alta | Alto | Faseamento; C++ e Python opcionais; gates | Cortar camadas | [02-roadmap-phases.md](02-roadmap-phases.md) |
| R9 | Metas de performance da investigação não se confirmam | Média | Médio | Benchmarks automatizados desde Fase 1 | Rebaixar metas e documentar | [07-quality-ci-release.md](07-quality-ci-release.md) |
| R10 | Assinatura/notarização (Apple, Windows) e auto-update atrasam release | Média | Médio | Configurar CI de release cedo | Distribuição manual no beta | [07-quality-ci-release.md](07-quality-ci-release.md) |

## Spikes

Formato: hipótese → experimento → critério go/no-go → timebox. Resultado vira ADR.

### S1 · Python embutido (PyO3 + subinterpretadores)
- **Hipótese**: dá para rodar N subinterpretadores em paralelo com GIL próprio, trocando tabelas Arrow zero-copy com Rust.
- **Experimento**: crate Rust com PyO3; Python 3.12/3.13; 4 subinterpretadores em threads Tokio executando função numérica; importar `pyarrow`/`numpy`; medir throughput vs 1 interpretador e vs sidecar HTTP.
- **Go**: ganho de paralelismo ≥ 2× com 4 workers **e** libs necessárias importam sem erro. **No-go**: usar worker externo ou free-threaded; atualizar ADR-003.
- **Extra**: medir tamanho do bundle por SO.
- **Timebox**: 3–4 dias.

### S2 · Wasmtime + Component Model + WASI
- **Hipótese**: componentes com imports WIT customizados, fuel e limite de memória funcionam; async nativo (0.3) disponível e estável.
- **Experimento**: host Rust com Wasmtime; componente em Rust e em C/Go; importar função async do host; esgotar fuel e memória; medir instanciação AOT.
- **Go**: limites aplicados de forma confiável e instanciação < poucos ms. **No-go (parcial)**: cair para WASI 0.2 com async do lado do host.
- **Timebox**: 3–4 dias.

### S3 · Tauri v2: Isolation Pattern + Binary IPC + multi-webview
- **Hipótese**: as três features coexistem e Binary IPC mantém ganho com isolation ativo.
- **Experimento**: app Tauri com 2–3 WebviewWindows; comando que devolve 10 MB binário; medir JSON vs binário com/sem isolation; testar `BroadcastChannel` entre janelas nos 3 SOs.
- **Go**: ganho ≥ 3× mantido com isolation; broadcast funciona nos 3 SOs. **No-go**: ajustar ADR-005/006.
- **Timebox**: 3 dias.

### S4 · `cxx` + `cxx-async` cross-OS
- **Hipótese**: bridge segura funciona com toolchains padrão nos 3 SOs, com exceção→`Result` e async C++20↔Tokio.
- **Experimento**: hello-world com `std::string`, `std::vector`, `unique_ptr`, exceção, corrotina `co_await`; compilar no CI 3 SOs.
- **Go**: build reproduzível nos 3 SOs. **No-go**: adiar C++ (Fase 4 cortada ou virar módulo Wasm).
- **Timebox**: 2–3 dias (pode ser adiado para antes da Fase 4).

### S5 · Baseline de performance do host
- **Hipótese**: Tauri atinge cold start/RAM/tamanho próximos dos alvos.
- **Experimento**: app mínimo; medir nos 3 SOs; comparar com tabela da investigação.
- **Timebox**: 1 dia.

## Resultados

### S5 · Baseline de performance (Windows 11, WebView2 Edge 157, Tauri 2.12, release com LTO/strip) — 2026-11-10
App mínimo de 1 janela em `spikes/s5-baseline/`. Start = do `Start-Process` até a página chamar o host (mediana de 5 execuções, 1ª descartada).

| Métrica | Alvo | Medido | Veredito |
|---|---|---|---|
| Cold start | 0,3–0,8 s | **376 ms** (1ª execução fria: 558 ms) | ✅ dentro |
| Tamanho do executável | instalador ~4–5 MB | **3,0 MB** (instalador não medido) | ✅ provável |
| RAM ociosa (host + 6 processos WebView2) | 25–40 MB | **~310–330 MB** working set; **~150 MB** privada | ❌ muito acima |
| Binary IPC vs JSON (S3) | 5–10× | 4,4–4,7× | ⚠️ pouco abaixo |

- **Conclusão**: a meta de RAM da investigação não é realista se contada como o usuário vê no Gerenciador de Tarefas (o WebView2 sobe 6 processos). Ela só faria sentido para o processo host isolado, que **não foi medido separadamente**. **Ação**: redefinir a meta como "RAM total privada ≈ 150 MB por janela base" ou medir host e WebView separados, e documentar em [00-vision-scope.md](00-vision-scope.md).
- Cada janela extra do WebView2 compartilha processos, mas adiciona RAM; medir na Fase 1 com o shell + 2 micro-frontends.
- **Pendente**: Linux e macOS (ver [TODO](TODO.md)).

### S2 · Wasmtime + Component Model — 2026-11-10 (Windows)
**Go parcial.** Instanciação ~36 µs, fuel e limite de memória contidos, FS/rede negados sem permissão. Usar WASI 0.2 + async no host; WASI 0.3 não testado. Detalhes e ressalvas no [ADR-004](04-adrs/ADR-004-wasm-miniapps.md).

### S1 · Python embutido — 2026-11-10 (Windows)
**No-go para subinterpretadores embutidos.** 2,53× com 4 subinterpretadores, mas `numpy`/`pyarrow` não carregam; 4 processos dão 3,34× com as libs funcionando. R1 confirmado. Decisão: worker em processo filho + Arrow IPC ([ADR-003](04-adrs/ADR-003-python-embedded-vs-sidecar.md)).

## Ordem sugerida

S3 → S5 → S2 → S1 → S4 (S4 só se a Fase 4 permanecer no plano).
