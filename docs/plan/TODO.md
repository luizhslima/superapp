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
- ⏳ Criar remoto no GitHub (repo público para CI de graça nos 3 SOs) e fazer o primeiro push

## Dívidas técnicas conhecidas
- ⏳ S3: o IPC binário rodou a ~110 MB/s para 10 MB. Investigar se o gargalo é a geração do `Vec` por chamada ou o transporte do Tauri antes de usar como teto.
- ⏳ `core:default` no spike S3 é permissivo demais; na Fase 1 usar capabilities mínimas por janela (ADR-006).
- ⏳ pnpm quebrado no Git Bash (caminho do Anaconda no PATH); usar PowerShell até corrigir.

## Decisões de custo zero a revisar
- Assinatura de código (Windows) e notarização (Apple Developer, US$ 99/ano) **não** são gratuitas: adiar para a Fase 5 e distribuir sem assinatura no beta (R10).
- Telemetria/registro de mini-apps (Fase 5) precisará de hospedagem: avaliar GitHub Releases/Pages como registro estático.

## Próximos passos (ordem)
1. S3 + S5 no Linux
2. Gate da Fase 0 (aceitando Windows-only + ADR-007 adiado, ou depois do Linux)
3. Iniciar Fase 1 (casca segura) e reavaliação do roadmap
