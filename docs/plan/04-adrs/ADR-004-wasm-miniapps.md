# ADR-004 · WebAssembly (Wasmtime + Component Model) para mini-apps

- **Status**: Accepted (versão WASI condicionada ao spike S2)
- **Contexto**: Código de terceiros exige isolamento forte e arranque quase nulo. Docker é pesado (100 ms–2 s, 50–200 MB); V8 isolates limitam a JS.
- **Decisão**: Mini-apps como componentes Wasm no Wasmtime, com API do host definida em WIT, fuel metering e limites de memória; AOT (Cranelift) para arranque rápido.
- **WASI**: alvo WASI 0.3 (async nativo). Se S2 mostrar imaturidade, usar WASI 0.2 com async tratado no host.
- **Consequências**: (+) poliglota, sandbox por capacidade, baixo custo. (−) ecossistema/toolchains em evolução; UI do mini-app precisa de estratégia separada (renderização via host/WIT ou webview restrita).
- **Questão em aberto**: como o mini-app renderiza UI (comandos declarativos via WIT vs webview isolada com CSP). Decidir na Fase 2.
- **Riscos**: R3, R7.
