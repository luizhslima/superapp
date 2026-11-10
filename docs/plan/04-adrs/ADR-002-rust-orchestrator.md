# ADR-002 · Rust como orquestrador central

- **Status**: Accepted
- **Contexto**: O core gerencia IPC, capabilities, ciclo de vida e hospeda Wasm/Python/C++. Precisa de segurança de memória e async maduro.
- **Decisão**: Core em Rust com Tokio como runtime assíncrono; sem servidor local persistente nem portas localhost.
- **Consequências**: (+) elimina classes de vulnerabilidades de memória; integra com Tauri, Wasmtime, PyO3, cxx. (−) curva de aprendizado; compilação lenta (mitigar com workspace e cache).
- **Alternativas**: Node.js (descartado), Go (sem integração nativa com Tauri/Wasmtime).
