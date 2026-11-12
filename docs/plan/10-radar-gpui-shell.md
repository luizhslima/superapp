# Radar: casca alternativa em GPUI (spike + beta paralela)

**Status:** no radar, **não iniciado** (decidido em 2026-11-12). Nenhuma decisão de migrar.
O Tauri continua sendo a casca oficial ([ADR-001](04-adrs/ADR-001-tauri-v2.md)).

## Ideia

[GPUI](https://gpui.rs/) (framework de UI em Rust, com renderização em GPU, do editor Zed) como
**segunda casca** do Superapp, ao lado da atual em Tauri, sobre o mesmo núcleo. Motivação:
faixa de abas, menus e toolbar realmente nativos (no Tauri, no Windows/Linux, abas só existem
desenhadas em HTML ou com a API instável de multi-webview, que foi recusada), e menos memória
por não ter WebView.

## Por que é viável

O núcleo já não depende do Tauri: `crates/core` (Router/política), `crates/package` (pacotes
e catálogo assinados), `crates/wasm-host` (Wasmtime, ciclo de vida, backup) e `crates/ui` (UI
declarativa, ADR-009). Só o shell é específico do Tauri (`src-tauri`, `ui/shell`). A UI
declarativa é agnóstica de renderizador: a mesma árvore pode ser desenhada por HTML ou GPUI.

## Riscos e custos conhecidos

- GPUI é **pré-1.0**: API instável e pouca documentação (mesma classe de risco do multi-webview).
- Perde-se o que o Tauri dá pronto: auto-update (refazer mantendo a assinatura minisign),
  diálogos de arquivo, instaladores e o pipeline de release.
- Acessibilidade (leitores de tela), campos de texto (IME, seleção, colar) e build pesado nos 3
  sistemas (macOS só pela CI).
- Manter duas cascas dobra o trabalho de telas: antes de uma beta real, extrair a lógica que
  hoje vive em `src-tauri` (`store`, `trust`, `data`, `diag`) para um crate comum sem Tauri.

## Plano proposto (em ordem; cada passo só se o anterior justificar)

1. **Spike isolado e descartável** (crate fora do workspace, como `spikes/`): janela GPUI com
   abas desenhando Clicker e Tarefas pelo mesmo host Wasm. Mede: compila/roda no Windows?,
   tempo de build, memória e inicialização vs. o app atual, esforço de abas/campos/listas.
   Sai um relatório curto com a decisão (migrar / não migrar / só mais tarde). O app atual
   não é tocado.
2. **Extrair o crate comum** (lógica sem Tauri) e montar a casca GPUI de verdade.
3. **Beta paralela opt-in**: segunda casca como pré-lançamento em tags próprias (ex.:
   `gpui-v0.1.0-beta`) e canal de atualização separado; a Tauri segue como estável. Dados
   (mini-apps, chaves, backups) compartilhados com bloqueio simples para não abrir as duas
   ao mesmo tempo, ou pastas separadas.

## Critérios para decidir

- O spike roda no Windows sem gambiarras e com build aceitável.
- Memória/inicialização melhores que o app atual de forma mensurável.
- Abas, campos de texto e listas saem com esforço razoável.
- O custo de manter duas cascas é aceitável para um projeto solo.

Se qualquer um falhar, registra-se o resultado aqui e o Tauri segue sozinho.
