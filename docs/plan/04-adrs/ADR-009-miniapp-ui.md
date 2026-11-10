# ADR-009 · UI de mini-apps: comandos declarativos (v0)

- **Status**: Accepted (2026-11-10) para o v0; webview isolada fica como evolução futura
- **Contexto**: o ADR-004 deixou em aberto como um mini-app Wasm mostra interface. Duas opções foram avaliadas:
  - **(a) Comandos declarativos via WIT**: o mini-app devolve uma árvore de componentes de um conjunto fixo (texto, botão, lista) e o shell a renderiza; eventos de volta pelo WIT.
  - **(b) Webview isolada por mini-app** com CSP própria: o mini-app traz HTML/JS e o Wasm cuida da lógica.
- **Decisão**: **(a) no v0.** O mini-app nunca executa código na página: só descreve a interface, e o shell (código nosso) a renderiza.
- **Por quê**: superfície de ataque mínima (nada de HTML/JS de terceiros dentro do host), reaproveita o modelo de segurança já validado na Fase 1 (Router, política por janela, auditoria), é suficiente para os mini-apps de exemplo e mantém o custo de uma pessoa só. (b) multiplica a superfície (CSP por app, vazamento entre webviews, diferenças WebView2/WKWebView/WebKitGTK, R6).
- **Consequências**: (+) UI uniforme, acessibilidade e tema controlados pelo host; sem XSS vindo de mini-app. (−) expressividade limitada ao catálogo de componentes; apps que precisam de canvas/WebGL ficam fora até (b).
- **Revisitar quando**: houver um caso de uso real que o catálogo não cobre. Aí abrir novo ADR para (b), com mini-app em webview própria, sem acesso ao `ipc` do shell e com canal restrito ao seu Wasm.
- **Implementação**: não faz parte da entrega 1 da Fase 2 (que entrega o host Wasm, sem UI). O catálogo de componentes e a interface WIT de UI entram junto com o ciclo de vida e o consentimento (E2.4).
- **Riscos**: R6, R7.
