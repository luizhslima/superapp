# ADR-001 · Tauri v2 como framework de host

- **Status**: Accepted
- **Contexto**: Electron entrega uniformidade mas com binário ~165 MB e RAM 90–180 MB (investigação). Precisamos de core leve, seguro, com WebView nativo.
- **Decisão**: Usar Tauri v2 (Rust + WebView do SO).
- **Consequências**: (+) binário e RAM menores, modelo de capabilities, Binary IPC, multi-webview. (−) fragmentação de WebView (WebView2/WKWebView/WebKitGTK) exige testes nos 3 SOs; ecossistema menor que Electron.
- **Alternativas**: Electron (descartado por peso/segurança padrão); Qt/C++ (mais pesado de dev, menos ecossistema web).
- **Riscos**: R6. **Validação**: S3, S5.
