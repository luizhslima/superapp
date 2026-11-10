# ADR-004 · WebAssembly (Wasmtime + Component Model) para mini-apps

- **Status**: Accepted — WASI 0.2 com async no host (S2, 2026-11-10); WASI 0.3 a reavaliar
- **Contexto**: Código de terceiros exige isolamento forte e arranque quase nulo. Docker é pesado (100 ms–2 s, 50–200 MB); V8 isolates limitam a JS.
- **Decisão**: Mini-apps como componentes Wasm no Wasmtime, com API do host definida em WIT, fuel metering e limites de memória; AOT (Cranelift) para arranque rápido.
- **WASI**: **WASI 0.2 com async tratado no host** (resultado do S2). WASI 0.3 (async nativo) fica como evolução futura.
- **Consequências**: (+) poliglota, sandbox por capacidade, baixo custo. (−) ecossistema/toolchains em evolução; UI do mini-app precisa de estratégia separada (renderização via host/WIT ou webview restrita).
- **Questão em aberto**: como o mini-app renderiza UI (comandos declarativos via WIT vs webview isolada com CSP). Decidir na Fase 2.
- **Riscos**: R3, R7.

## Resultado do spike S2 (2026-11-10, Windows 11, Wasmtime 50.0.1, guest Rust em `wasm32-wasip2`)
Código em `spikes/s2-wasmtime/` (WIT, guest e host).

| Teste | Resultado |
|---|---|
| Instanciação a partir de componente AOT pré-compilado (`InstancePre`) | **~36 µs** (média de 1000) |
| Precompilação (Cranelift), 135 KB wasm → 593 KB nativo | 37,5 ms (custo único, fora do caminho quente) |
| Import WIT async implementado no host (`tokio::sleep`) | OK |
| Loop infinito com fuel = 5M | trap em ~0,5 ms |
| Alocar 64 MB com limite de 16 MB | contido (trap) |
| Alocar 4 MB com limite de 16 MB | OK |
| `read_dir("/")` sem preopens | negado |
| `TcpStream::connect` sem permissão de rede | negado |

- **Decisão revisada (go parcial)**: usar **WASI 0.2 (wasip2) com async tratado no host** (`imports/exports: async` no `bindgen!`). Os limites de fuel e memória são aplicados de forma confiável. O async nativo do WASI 0.3 **não foi testado** (ainda atrás de feature experimental); reavaliar quando estabilizar. O formato de componente e o WIT permitem migrar sem mudar o contrato.
- **Não coberto**: guest em C/Go (o plano pedia); o erro de OOM aparece como `unreachable` do allocator do guest, não como erro de limite do host, então o SDK deve traduzir isso em mensagem útil. Um loop sem fuel (`consume_fuel` desligado) não é interrompido; ligar fuel é obrigatório (checar no loader).
