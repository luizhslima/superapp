# TODO · Pendências e decisões em aberto

Restrição do projeto: **custo zero**. Só há máquina Windows 11 e Linux; **sem macOS por enquanto** (compra planejada). Atualizar este arquivo a cada sessão.

Legenda: ⏳ pendente · 🔄 em andamento · ✅ feito · 🧊 bloqueado/adiado

## Fase 0 · Fundação e Spikes
- ✅ Instalar Rust (1.99.1, MSVC) e preparar toolchain
- ✅ Scaffold do monorepo (Cargo workspace, xtask, pnpm, CI matriz)
- ✅ S3 no **Windows**: go (4,4× com isolation). Ver [ADR-005](04-adrs/ADR-005-binary-ipc-arrow.md)
- ✅ S5 no **Windows**: cold start 376 ms ✅, exe 3 MB ✅, RAM ~150 MB privada ❌ (meta 25–40 MB irreal). Ver [03](03-risks-and-spikes.md)
- ⏳ Revisar a meta de RAM em [00-vision-scope.md](00-vision-scope.md) e medir host isolado vs WebView2
- ✅ S2 (Windows): go parcial, WASI 0.2 + async no host. Ver [ADR-004](04-adrs/ADR-004-wasm-miniapps.md)
- ⏳ S2 pendências: guest em C/Go, testar WASI 0.3 quando estabilizar, rodar em Linux/macOS
- ✅ S1 (Windows): **no-go** subinterpretadores; adotado worker em processo filho + Arrow IPC. Ver [ADR-003](04-adrs/ADR-003-python-embedded-vs-sidecar.md)
- ⏳ S1 pendências: medir latência/throughput do Arrow IPC com o worker (início da Fase 3); testar 3.13 free-threaded só se necessário; Linux/macOS
- 🧊 S4: cxx/cxx-async (só se a Fase 4 continuar no plano)
- ✅ ADRs 003/004/005/008 → *Accepted*; 001/002/006 já estavam. **ADR-007 (C++) segue *Proposed* de propósito** (S4 adiado); o gate da Fase 0 deve aceitar essa exceção.
- ✅ Arquitetura (01), backlog (05), estrutura (08) e README atualizados com as decisões de S1/S2/S3

## Validação multi-SO (pendente)
- ⏳ **Linux**: rodar S3 e S5 na máquina Linux local (precisa de `webkit2gtk-4.1`, `libgtk-3-dev`, `libayatana-appindicator3-dev`). Registrar números no ADR-005 e no relatório do S5.
- 🧊 **macOS**: sem hardware. Todo resultado de macOS fica marcado **"não validado"** até a compra da máquina.
  - S3 (BroadcastChannel + binary IPC com isolation em WKWebView), S5, empacotamento, assinatura/notarização.
  - Alternativa gratuita enquanto isso: runner `macos-latest` do GitHub Actions (grátis em repo **público**; sem GUI interativa, mas serve para build, testes e talvez smoke test headless). Requer criar o remoto no GitHub.
- ✅ Remoto `luizhslima/superapp` com `main` publicada; **CI verde em Linux, Windows e macOS** (cargo xtask ci, 45 s)
- ✅ Repo agora **público** (2026-11-10): minutos de CI gratuitos, inclusive macOS. Cuidado: nunca commitar segredos (varredura inicial limpa).

## Dívidas técnicas conhecidas
- ⏳ S3: o IPC binário rodou a ~110 MB/s para 10 MB. Investigar se o gargalo é a geração do `Vec` por chamada ou o transporte do Tauri antes de usar como teto.
- ⏳ `core:default` no spike S3 é permissivo demais; na Fase 1 usar capabilities mínimas por janela (ADR-006).
- ⏳ pnpm quebrado no Git Bash (caminho do Anaconda no PATH); usar PowerShell até corrigir.

## Decisões de custo zero a revisar
- Assinatura de código (Windows) e notarização (Apple Developer, US$ 99/ano) **não** são gratuitas: adiar para a Fase 5 e distribuir sem assinatura no beta (R10).
- Telemetria/registro de mini-apps (Fase 5) precisará de hospedagem: avaliar GitHub Releases/Pages como registro estático.

## Fase 1 (em andamento)
- ✅ E1.1 `superapp-core`: roteador IPC tipado, política deny-by-default por janela, erros padronizados, auditoria (8 testes).
- ✅ App Tauri real (`src-tauri`): único comando `ipc` → `Router::dispatch`; identidade = label da janela (não vem da UI); Isolation hook com allowlist; CSP; smoke test de 2 janelas (main permitido, untrusted bloqueado, desconhecido, args inválidos) passando no Windows
- ✅ Hook rejeita nome inválido, comando >64 chars, args >64 KB, `plugin:fs|*` e `plugin:window|destroy` (testado via CDP)
- ✅ Rejeições do hook agora são **auditadas** (`DeniedByHook`): o hook reescreve a mensagem barrada para o comando reservado `HOOK_REJECTED` (maiúsculas: a UI não consegue produzi-lo) e o host registra o nome tentado, truncado em 64 chars e sanitizado. A promise da UI deixa de ficar pendurada: recebe `permission_denied`
- ✅ Auditoria persistente: `FileAudit` grava JSONL em `<log_dir>/audit.jsonl` (Windows: `%LOCALAPPDATA%\dev.superapp.app\logs`). Pendente: rotação/limite de tamanho
- ✅ Smoke automatizado: `cargo xtask smoke` (14 verificações) passa **nos 3 SOs na CI** (Windows, Linux via xvfb, macOS), 14/14. Isso valida o hook de isolamento, a CSP, o `BroadcastChannel` entre janelas e a política por janela em WebKitGTK e WKWebView. Dívida restante de hardware: macOS real (desempenho/empacotamento) e S3/S5 fora do Windows
- ✅ CSP: o aviso vinha do estilo inline que o Tauri aplica no iframe de isolamento; liberado só o hash exato via `'unsafe-hashes'` (sem `unsafe-inline`). Console limpo
- ⚠️ `window.ipc` é reservado pelo WebView (wry); usar `window.host`
- ⏳ E1.2 capabilities mínimas por janela + isolation hook que valida payload
- ✅ E1.3 shell + 2 micro-frontends (`mfe-a`, `mfe-b`) em janelas próprias: aberturas por `shell.open_mfe` (cap `shell:manage`, só ids `a`/`b`), cada MFE com só `app:read`, comunicação por `BroadcastChannel`. Com o A em laço infinito, o shell fala com o host em 2–8 ms e o B segue respondendo; 18/18 no smoke nos 3 SOs. Limite do teste: trava só a thread da página (`for(;;){}`), não mede consumo de memória nem crash do processo do WebView
- ✅ E1.4 binary IPC pelo Router (`register_bytes`/`dispatch_bytes`: capability + limite de entrada e de **saída** + auditoria `DeniedTooLarge`). Dois canais:
  - `ipc_bytes` (Tauri IPC, passa pelo hook de Isolation): ok, mas **UI→host ~1 MiB/s** (2 MiB = 2,2 s; linear: custo por byte do Isolation). Por isso o hook limita este canal a **1 MiB**; acima disso, `sapp://`.
  - `sapp://localhost/<comando>` (esquema próprio, fora do hook; identidade = `webview_label`): **envio 10 MiB em ~155 ms (~65 MiB/s), ~85× mais rápido**. Usar este para corpos grandes.
  - **Investigação dos ~110 MB/s (host→UI)**: 10 MiB em ~85–90 ms nos dois canais, com ou sem Isolation, e gerar o `Vec` custa ~1 ms. Logo o teto é o transporte do WebView2 (Windows), não o nosso código. Suficiente para a meta; dados maiores devem ir por arquivo/stream. Medido na CI (runners, 10 MiB): host→UI Windows ~115 ms, Linux ~62 ms, macOS ~14–123 ms; `sapp://` envio Windows ~157 ms, Linux ~22 ms, macOS ~10 ms; `ipc_bytes` envio de 1 MiB ~1,0–1,6 s nos três. Ou seja: o teto de ~110 MiB/s é **específico do WebView2/Windows**; o Isolation custa ~1 µs/byte em todos
  - Risco aceito: `sapp://` recebe o corpo inteiro em memória antes do Router checar o limite (o Tauri não expõe Content-Length antes). Só código do próprio app roda nas janelas; reavaliar quando houver conteúdo de terceiros em janelas
- ✅ E1.5 empacotamento + benchmark: workflow `package.yml` (manual e tags `v*`) gera instaladores **sem assinatura** nos 3 SOs e `cargo xtask bench` mede o release. Resultados (runners CI, 2026-11-10):

  | SO | instalador | binário | host RSS | UI pronta |
  |---|---|---|---|---|
  | Windows | NSIS 1,9 MB | 8,7 MB | ~30 MB (total WebView2 ~350 MB*) | ~0,5–4 s |
  | Linux | deb 3,0 MB · AppImage 78 MB | 11,3 MB | ~126 MB | ~32 s (xvfb, sem GPU; não representativo) |
  | macOS | dmg 2,9 MB (aarch64) | 9,9 MB | ~94 MB (WKWebView fica fora da árvore; subestima) | ~5,4 s |

  *Soma de RSS dos processos: superestima (páginas compartilhadas contadas várias vezes). Só o NSIS foi gerado e **não instalado** localmente (instalar altera o registro do usuário). Instaladores dos outros SOs não foram executados, só construídos.
  - **Meta de RAM (00-vision-scope.md)**: o host Rust sozinho fica em ~30 MB no Windows (dentro de 25–40 MB), mas o total com WebView2 é bem maior. Meta revisada: host ≤ 40 MB e WebView medido à parte. Medição confiável de memória real exige máquina física (Working Set privado), não RSS somado
  - Orçamento de "UI pronta" é só aviso (> 15 s): o CI sem GPU distorce o tempo

## Fase 2 (em andamento)
- ✅ Entrega 1: `crates/wasm-host` (E2.1 + parte do E2.2): engine com fuel obrigatório, concessão por interface WIT, limites de memória/tempo, storage isolado por mini-app, auditoria `app:<id>`, mini-app de exemplo `miniapps/counter`; 12 testes de ataque (loop, memória, FS/rede, timeout com host responsivo, import não concedido, path traversal, quota) em `cargo test`. Os guests compilam para `wasm32-wasip2` durante o teste (a CI instala o target)
- ✅ ADR-009: UI de mini-apps declarativa no v0
- ✅ Entrega 2 (E2.3): `crates/package` (container `.sapp`, manifesto estrito, Ed25519 + SHA-256, `TrustStore`), `Runtime::load_verified` (aprovado ⊆ declarado; import além do declarado é recusado), `cargo xtask keygen|pack|verify`. Testes: 12 no crate + 5 de ponta a ponta com o `counter` real. Cobrem manifesto adulterado, wasm adulterado/trocado, assinatura corrompida, chave não confiável, troca de chave pública, assinatura sem prefixo de domínio, tamanhos absurdos (4 GiB) e bytes sobrando/faltando, campo/permissão desconhecidos e limites acima do teto. ADR-010
- ⏳ Pendente do pacote: anti-rollback de versão, revogação/rotação de chaves, onde fica a `trusted-keys.txt` do app (decidir na entrega 3)
- ✅ Entrega 3 (E2.4): `lifecycle::Registry` (inspecionar → consentir → instalar/atualizar; suspender/retomar; remover com ou sem dados; reverificação do pacote a cada carga, então chave removida da `TrustStore` bloqueia o app; anti-rollback; aprovado ⊆ declarado), catálogo de UI declarativa validado (`ui.rs`), integração no Tauri (`apps.*` no Router, capability `apps:manage` só na janela principal; `.sapp` chega por arrastar-e-soltar tratado no Rust, a UI nunca informa caminhos; `ipc` virou assíncrono com `spawn_blocking`), tela de consentimento/lista/visão do mini-app, exemplo `miniapps/clicker`. Testes: 8 de ciclo de vida + 4 de UI; smoke +10 verificações do fluxo completo; tela real dirigida por CDP (consentimento, instalar, abrir, clicar, suspender, remover; console sem erros de CSP)
- ⚠️ **Tamanho**: embutir Wasmtime levou o binário de 8,7 para 24,1 MB e o NSIS de 1,9 para 5,55 MiB (já com LTO, strip e features do Wasmtime cortadas). Opções para reduzir, se importar: (1) runtime só-AOT sem Cranelift (−~15 MB), exigindo pacotes com código nativo pré-compilado por plataforma e assinados (muda o ADR-010: o `.sapp` deixa de ser portátil); (2) manter o Cranelift e aceitar ~6 MiB; (3) baixar o runtime Wasm sob demanda como módulo opcional. **Decidido em 2026-11-10: opção (2), aceitar ~6 MiB com Cranelift.** Revisitar só se a distribuição mostrar que o tamanho atrapalha
- ⏳ Pendente da entrega 3: executar/remedir dmg, deb e AppImage com o runtime; arrastar-e-soltar real não foi exercitado em CI (só o caminho equivalente por variável de ambiente e a tela via CDP); onde fica a `trusted-keys.txt` em produção (hoje `<dados do app>/trusted-keys.txt`, sem UI para gerenciar)
- ⏳ Entrega 3 (antigo): ciclo de vida + consentimento + UI declarativa (E2.4) · entrega 4: SDK, 2º mini-app (C/Go) e suíte completa (E2.5/E2.6)
- ⏳ Cache AOT em disco; integrar o `wasm-host` ao app Tauri (comando no Router) na entrega 3

- ✅ Entrega 4a (E2.5/E2.6): `superapp-ui` (tipos de UI compartilhados host/SDK), **SDK Rust** (`miniapps/sdk`: `trait App`, `export_app!`, helpers de UI/storage; o `clicker` caiu para ~12 linhas de lógica), guia [docs/dev/miniapps.md](../dev/miniapps.md), detector de segredos no `pack`, suíte `ui_attacks` (6 modos de UI maliciosa, trap, limites do manifesto, isolamento entre apps) e matriz de ataques em `06-security-model.md`
- ✅ Entrega 4b (E2.5): **2º idioma = JavaScript** (`miniapps/clicker-js`, `jco componentize` 1.41.0 fixo), instalado e executado de ponta a ponta no mesmo host (`tests/js_guest.rs`, obrigatório na CI via `SUPERAPP_REQUIRE_JS`). Achados: (1) o `jco` não poda importações, então o app declara um mundo WIT enxuto; (2) o interpretador embute `wasi:http`, que o host nunca liga, e é preciso `--disable http fetch-event`; (3) 11,5 MB e ~8,5 s para compilar, agora feito **na instalação**; depois ~4–7 ms e ~9,9 M de fuel por execução (cabe nos limites padrão). C/Go: sem toolchain local (wasi-sdk/TinyGo), não tentado
- ⏳ **Cache de componentes compilados em disco**: hoje, após reiniciar o app, a 1ª abertura de um app grande (JS) recompila (~8,5 s). Cache exige `Component::deserialize` (unsafe) sobre código nativo gravado em disco: só é seguro se autenticado (ex.: chave do host + hash do wasm + versão do Wasmtime). Alternativas: pré-compilar em segundo plano no boot, ou `--aot`/QuickJS do jco para componentes menores
- ⏳ Perfil `dev` agora otimiza Cranelift/Wasmtime (`opt-level = 3`): compilar o componente JS caiu de ~100 s para ~9 s nos testes

## Fases 3 e 4: adiadas (2026-11-10)
Sem caso de uso concreto de IA/dados (Fase 3) nem de código nativo (Fase 4). Retomar quando houver um. Pendências que ficam registradas: S4 (`cxx` cross-OS), medição de latência Arrow IPC (início da Fase 3), WASI 0.3.

## Fase 5 (próxima)
- ⚠️ **Lacuna do gate da Fase 2**: `07-quality-ci-release.md` lista `cargo-audit`, `cargo-deny` e fuzz de parsers (manifesto/pacote/UI/IPC) como parte da segurança do gate da Fase 2. **Não foram feitos.** Entram primeiro na Fase 5 (E5.4)
- **Decisões (2026-11-10)**: telemetria = **relatório de diagnóstico local** (nada sai sozinho; o usuário vê e envia se quiser); distribuição = **catálogo estático assinado** (JSON assinado por nós em hospedagem gratuita; exige rede HTTPS restrita ao endereço do catálogo, auditada); começo = **endurecimento primeiro**
- ✅ **E5.4 endurecimento (2026-11-10)**: `cargo-deny` na CI (`deny.toml`; advisories/licenças/bans/fontes ok), fuzz por mutação nos parsers (pacote, manifesto, UI, roteador), 4 alvos `cargo-fuzz` em workflow semanal, SBOM CycloneDX, **2 achados corrigidos** (nomes hostis no log de auditoria; log sem limite → rotação 5 MiB × 3). Relatório: [docs/security/self-audit.md](../security/self-audit.md). Falta validar na CI os workflows `fuzz` e `sbom`
- 🟡 **E5.2 auto-update (2026-11-11)**: par de chaves minisign gerado (privada + senha em `G:\dev\secrets`, **backup fora do PC é do usuário**; pública em `docs/security/updater-pubkey.txt` e `tauri.conf.json`); `release.yml` em tags `v*` (rascunho de Release com instaladores assinados + `latest.json`); app: `app.update.check/install` só pelo Router (capability `app:update`, só `main`), endereço e chave fixos no config, HTTPS obrigatório, botão na tela principal, negado para janela sem capability no smoke. **Falta**: cadastrar os 2 segredos no GitHub, criar a primeira tag (ex.: v0.1.0) e testar de ponta a ponta (instalar v0.1.0, publicar v0.1.1, atualizar). Auto-update nunca foi exercitado de verdade
- ✅ **E5.2 auto-update (2026-11-11)**: validado de ponta a ponta no Windows (v0.1.2 instalada → v0.1.3 via botão, assinatura minisign verificada). Lições: segredos ficam no environment `CI_SUPERAPP_ENVS` (job declara `environment`); matriz em série (`max-parallel: 1`) para o `latest.json` manter as 3 plataformas. **Não testado**: atualização em Linux/macOS (só o Windows foi exercitado). Guia: [docs/dev/release.md](../dev/release.md)
- ✅ **E5.3 diagnóstico local**: `diag.report` (capability `diag:read`, só `main`), sink de tamanho fixo (contadores + 50 últimas negações), botão na tela, nada sai sozinho; negado para janela sem capability (smoke)
- ✅ **E5.1 catálogo assinado (2026-11-11)**: validado de ponta a ponta no Windows (v0.1.4: carregar catálogo → baixar → consentimento → instalar → abrir/usar o Clicker). Catálogo em `catalog/catalog.json`, pacote na Release `apps-1`, publicador `e993326b…dab7` (chave privada em `G:\dev\secrets\superapp-publisher.key`, **backup fora do PC**; a pública entra no `trusted-keys.txt` de quem usa). **Manutenção**: o catálogo **expira em 90 dias** (reassinar com `cargo xtask catalog`; ainda sem lembrete automático). Código: `store.rs`, `catalog.rs`, `xtask catalog`; `MemoryAudit` limitado. Falta: gerenciar `trusted-keys.txt` pela UI, lista de revogação, testar em Linux/macOS
- Fases de trabalho propostas: E5.4 endurecimento (audit/deny/fuzz/SBOM) → E5.2 pipeline de release + auto-update → E5.1 distribuição de mini-apps → E5.5 docs + E5.3 diagnóstico → E5.6 beta

## Fase 1: fechamento
E1.1–E1.5 concluídos. Pendências que sobram (não bloqueiam): (rotação do log de auditoria: feita em E5.4); medir S3/S5 e benchmark em máquinas reais (Linux local, macOS quando houver); instaladores nunca executados fora do Windows (CI só constrói); assinatura/notarização (Fase 5); meta de RAM **revisada** em `00-vision-scope.md` (host ≤ 40 MB; WebView medido à parte, ainda sem alvo).

## Gate da Fase 0
✅ Aceito em 2026-11-10 (Windows-only, ADR-007 adiado). Dívida: Linux/macOS na Fase 1.

## Próximos passos (ordem)
1. **Fase 1**: E1.1 core Rust + roteador IPC → E1.2 capabilities/isolation → E1.3 shell multi-janela → E1.4 binary IPC → E1.5 empacotamento
3. Rodar S3/S5 no Linux local quando conveniente
