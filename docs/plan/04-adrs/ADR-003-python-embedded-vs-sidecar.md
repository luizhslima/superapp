# ADR-003 · Python: embutido (PyO3) vs sidecar

- **Status**: **Accepted (2026-11-10) — opção 3: worker Python em processo filho gerenciado.** Ver "Resultado do spike S1".
- **Contexto**: A investigação recomenda CPython embutido via PyO3 com subinterpretadores (PEP 684) e Arrow zero-copy, rejeitando sidecar. Riscos: suporte PyO3 a multi-interpretador limitado; extensões C (NumPy) podem não suportar per-interpreter GIL; bundling do CPython nos 3 SOs pesa no instalador.
- **Decisão provisória**: Python é **módulo opcional** (baixado sob demanda), nunca no instalador base. Estratégia final depende de S1:
  1. **Go** → embutido com pool de subinterpretadores.
  2. **Parcial** → embutido em Python 3.13 free-threaded.
  3. **No-go** → worker Python em processo filho gerenciado (ciclo de vida pelo Rust, sem zumbis), transporte Arrow IPC (shared memory/pipe), não HTTP.
- **Consequências**: preserva a meta de tamanho; mantém a opção de otimização; adiciona complexidade de download/assinatura do runtime.
- **Riscos**: R1, R2.

## Resultado do spike S1 (2026-11-10, Windows 11, 8 núcleos/16 threads, CPython 3.12.10, numpy 2.5.4, pyarrow 26.0.1)
Código em `spikes/s1-python/` (Rust + `pyo3-ffi`, `Py_NewInterpreterFromConfig` com `PyInterpreterConfig_OWN_GIL`). Carga: 4 workers × 20M iterações de laço Python puro.

| Estratégia | Tempo | Speedup vs serial | numpy / pyarrow |
|---|---|---|---|
| Serial (1 interpretador) | 5,54 s | 1,00× | ok |
| 4 threads, 1 interpretador (GIL único) | 5,55 s | 1,00× | ok |
| **4 subinterpretadores, GIL próprio** | 2,19 s | **2,53×** | ❌ **ImportError** nos dois |
| **4 processos separados (multiprocessing)** | 1,54 s | **3,34×** | ✅ ok (worker sobe em 0,23 s) |

- O paralelismo dos subinterpretadores existe (2,53× ≥ 2×), mas as extensões C (`numpy`, `pyarrow.lib`) **recusam carregar** em subinterpretadores isolados ("does not support loading in subinterpreters"). Pelo critério do plano (paralelismo **e** libs importando), é **no-go** para o embutido com subinterpretadores.
- Processos separados dão **mais** paralelismo (3,34×) e funcionam com as libs. Isso confirma a opção 3.
- **Decisão**: worker Python como processo filho gerenciado pelo Rust (sem zumbis: Job Object no Windows, `PR_SET_PDEATHSIG` no Linux, kqueue/pipe no macOS), com Arrow IPC por pipe/memória compartilhada. Sem PyO3 no caminho principal.
- **Tamanho**: Python 3.12 completo ≈ 133 MB; numpy+pyarrow elevam o venv a ≈ 153 MB (com pip etc.). Reforça que Python seja **módulo opcional baixado sob demanda**; avaliar `python-build-standalone`/embeddable para reduzir.
- **Não testado**: Python 3.13 free-threaded (opção 2) — numpy/pyarrow ainda exigem wheels `cp313t`; reavaliar se a latência de IPC do worker se mostrar um problema real. Latência/throughput do Arrow IPC com o worker **ainda não medidos** (testar no início da Fase 3).
- **Pendente**: Linux e macOS.
