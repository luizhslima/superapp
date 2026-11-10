# ADR-004 · WebAssembly (Wasmtime + Component Model) para mini-apps

- **Status**: Accepted — WASI 0.2 com async no host (S2, 2026-11-10); WASI 0.3 a reavaliar
- **Contexto**: Código de terceiros exige isolamento forte e arranque quase nulo. Docker é pesado (100 ms–2 s, 50–200 MB); V8 isolates limitam a JS.
- **Decisão**: Mini-apps como componentes Wasm no Wasmtime, com API do host definida em WIT, fuel metering e limites de memória; AOT (Cranelift) para arranque rápido.
- **WASI**: **WASI 0.2 com async tratado no host** (resultado do S2). WASI 0.3 (async nativo) fica como evolução futura.
- **Consequências**: (+) poliglota, sandbox por capacidade, baixo custo. (−) ecossistema/toolchains em evolução; UI do mini-app precisa de estratégia separada (renderização via host/WIT ou webview restrita).
- **UI do mini-app**: decidido no [ADR-009](ADR-009-miniapp-ui.md) (comandos declarativos no v0).
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

## Implementação (Fase 2, entrega 1, 2026-11-10)
Crate `crates/wasm-host` (sem Tauri) + WIT v0 em `crates/wasm-host/wit/host.wit` (`log`, `storage`, `clock`; export `run(input) -> result<string,string>`). Decisões de implementação:
- **Engine criada pelo `Runtime`, com `consume_fuel` sempre ligado**: não existe API para carregar com engine alheia, então a exigência "o loader recusa engine sem fuel" é garantida por construção.
- **Permissão = interface ligada ao linker**: só o que o manifesto concede. Importar uma interface não concedida faz `load` falhar com `ImportNotGranted` (auditado como `DeniedNoCapability`). O wit-bindgen só deixa no componente as importações que o código realmente usa, então um mini-app só exige as permissões que de fato usa.
- **Limites por execução**: fuel, teto de memória (limitador próprio que lembra a negação, para traduzir o `unreachable` do alocador em `OutOfMemory { limit_bytes }`), timeout total e `fuel_async_yield_interval` (um guest que só calcula não monopoliza a thread; coberto por teste em runtime de thread única).
- **Storage por mini-app**: chave limitada a 100 bytes, valor a 1 MiB, quota de 4 MiB; em disco, o nome do arquivo é a chave em hexadecimal, então `..`/`/`/nomes reservados não saem do diretório; id do app restrito a `[a-z0-9-]{1,32}`.
- **Auditoria**: toda chamada guest→host vira evento com identidade `app:<id>`.
- **Ainda não feito**: cache de componentes pré-compilados (AOT) em disco (hoje compila a cada `load`; entra com o pacote, E2.3); WASI 0.3; guest em C/Go.
