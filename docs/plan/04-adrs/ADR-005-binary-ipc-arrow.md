# ADR-005 · Binary IPC (Tauri) e Apache Arrow

- **Status**: Proposed — validar em S3 (IPC) e S1 (Arrow)
- **Contexto**: Serialização JSON em IPC é custosa para payloads grandes. Investigação cita ganho de 5–10× com binário.
- **Decisão**: Usar JSON apenas para controle; payloads grandes via Binary IPC (`ArrayBuffer`). Entre Rust e Python, Arrow zero-copy/DLPack quando embutido; Arrow IPC se worker externo.
- **Consequências**: (+) latência/CPU menores. (−) contratos binários exigem versionamento e validação rígida no Rust (tamanho/limites).
- **Regra**: todo payload binário é validado (tamanho máximo, formato) antes de uso.
- **Riscos**: R5.
