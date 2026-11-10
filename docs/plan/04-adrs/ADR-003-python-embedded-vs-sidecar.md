# ADR-003 · Python: embutido (PyO3) vs sidecar

- **Status**: **Proposed — condicional ao spike S1**
- **Contexto**: A investigação recomenda CPython embutido via PyO3 com subinterpretadores (PEP 684) e Arrow zero-copy, rejeitando sidecar. Riscos: suporte PyO3 a multi-interpretador limitado; extensões C (NumPy) podem não suportar per-interpreter GIL; bundling do CPython nos 3 SOs pesa no instalador.
- **Decisão provisória**: Python é **módulo opcional** (baixado sob demanda), nunca no instalador base. Estratégia final depende de S1:
  1. **Go** → embutido com pool de subinterpretadores.
  2. **Parcial** → embutido em Python 3.13 free-threaded.
  3. **No-go** → worker Python em processo filho gerenciado (ciclo de vida pelo Rust, sem zumbis), transporte Arrow IPC (shared memory/pipe), não HTTP.
- **Consequências**: preserva a meta de tamanho; mantém a opção de otimização; adiciona complexidade de download/assinatura do runtime.
- **Riscos**: R1, R2.
