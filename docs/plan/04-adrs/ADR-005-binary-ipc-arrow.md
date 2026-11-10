# ADR-005 · Binary IPC (Tauri) e Apache Arrow

- **Status**: Accepted (2026-11-10) — S3 validado no Windows; Linux/macOS pendentes. Arrow: com worker externo (S1), usa-se Arrow IPC, não zero-copy em processo.
- **Contexto**: Serialização JSON em IPC é custosa para payloads grandes. Investigação cita ganho de 5–10× com binário.
- **Decisão**: Usar JSON apenas para controle; payloads grandes via Binary IPC (`ArrayBuffer`). Entre Rust e Python, **Arrow IPC com worker externo** (decisão do ADR-003; zero-copy/DLPack só valeria com Python embutido, refutado em S1).
- **Consequências**: (+) latência/CPU menores. (−) contratos binários exigem versionamento e validação rígida no Rust (tamanho/limites).
- **Regra**: todo payload binário é validado (tamanho máximo, formato) antes de uso.
- **Riscos**: R5.

## Resultado do spike S3 (2026-11-10, Windows 11 / WebView2 Edge 157, Tauri 2.12, release)
Payload de 10 MB, mediana de 5 execuções após warmup. Código em `spikes/s3-tauri-ipc/`.

| Modo | JSON | Binário (`ipc::Response`) | Ganho |
|---|---|---|---|
| Sem isolation | 430,8 ms | 91,3 ms | 4,7× |
| Isolation Pattern | 417,7 ms | 94,9 ms | 4,4× |

- `BroadcastChannel` entre 2 WebviewWindows: OK (Windows).
- **Go (Windows)**: ganho ≥ 3× mantido com isolation. **Pendente**: macOS e Linux (rodar na CI/máquina real).
- Observação: o binário ainda leva ~90 ms/10 MB (~110 MB/s), bem abaixo do ideal; investigar se o custo é o `Vec` de 10 MB gerado por chamada ou o transporte antes de tratar como teto.
