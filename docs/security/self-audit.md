# Autoauditoria de segurança (E5.4) — 2026-11-10

Escopo: tudo que existe até o fim da Fase 2 (core/Router, hook de isolamento, canais binários,
pacote `.sapp`, host Wasm, ciclo de vida, UI declarativa, shell). **Não é auditoria externa**:
auditor independente custa dinheiro e foge do orçamento zero. Quem revisou foi quem escreveu o
código (com IA), o que limita o que se enxerga; por isso o foco foi em automatizar verificações
que continuam valendo depois.

## O que passou a rodar na CI

| Controle | Onde | O que garante |
|---|---|---|
| `cargo-deny` (`deny.toml`) | job `supply-chain` em todo push | só licenças permissivas/MPL-2.0; falha em vulnerabilidade RustSec ou crate *yanked*; só crates.io; sem OpenSSL/native-tls por engano |
| Fuzz por mutação (`tests/mutation.rs` em `package`, `ui`, `core`) | `cargo test`, 3 SOs | qualquer bit alterado de um pacote válido é rejeitado (exaustivo); 30 mil mutações aleatórias sem pânico; UI aceita sempre respeita os limites; roteador nunca executa comando sem capability, para 50 mil entradas |
| Fuzz guiado por cobertura (`fuzz/`, libFuzzer) | workflow `fuzz` (semanal + manual) | 4 alvos: `package_verify`, `manifest_json`, `ui_parse`, `router_dispatch` |
| SBOM CycloneDX | workflow `package` | inventário das dependências Rust de cada execução (retenção 90 dias) |
| Suíte de ataques | `cargo test` | matriz em [06-security-model.md](../plan/06-security-model.md) |

`cargo-audit` **não** foi adotado: o `cargo-deny` consulta o mesmo banco RustSec, e duplicar
a ferramenta só repetiria os achados. (Tentativa de instalá-lo localmente também falhou.)

## Achados e correções

| # | Achado | Gravidade | Correção |
|---|---|---|---|
| 1 | Nomes de comando que chegam por `sapp://` **não passam pelo hook**: iam crus para o log de auditoria e para o console (quebras de linha, escapes ANSI, textos enormes) | Média (forja de linhas no log) | Toda negação sanitiza nome e identidade da janela (64 caracteres, `[A-Za-z0-9_.:|-]`); teste `hostile_names_never_reach_the_audit_log_raw` |
| 2 | O log de auditoria crescia **sem limite**: uma janela barulhenta enche o disco | Média (negação de serviço local) | `FileAudit` rotaciona por tamanho (5 MiB × 3 arquivos); teste `file_audit_rotates_by_size...` |
| 3 | Gate da Fase 2 listava `cargo-deny`/fuzz e não foram feitos | Processo | Feito agora (tabela acima) |

## Riscos residuais (aceitos ou pendentes)

- **`proc-macro-error` sem manutenção (RUSTSEC-2024-0370)**: dependência transitiva do Tauri no
  Linux (`gtk3-macros`), só em tempo de compilação. Não falha a CI (política `unmaintained =
  "workspace"`); reavaliar quando o Tauri atualizar o `gtk-rs`.
- **`sapp://` recebe o corpo inteiro em memória antes de checar o limite** (o Tauri não expõe o
  tamanho antes). Hoje só código do próprio app roda nas janelas; reavaliar se algum dia houver
  conteúdo de terceiros em webviews.
- **Canais laterais de tempo**: o relógio WASI está disponível ao mini-app.
- **Sem revogação central de chaves nem lista de revogação**: remover a chave de
  `trusted-keys.txt` bloqueia o app, mas não há distribuição dessa lista.
- **`unsafe` do Wasmtime e das dependências** não foi auditado por nós. No código do projeto,
  `unsafe_code = "deny"` vale para todo o workspace (só os spikes, descartáveis, usam `unsafe`).
- **Sem assinatura de código do SO** (Windows/macOS): o usuário verá avisos de SmartScreen e
  Gatekeeper. Mitigação possível só com certificado pago (fora do orçamento).
- **Cache de componentes compilados em disco** não existe justamente por exigir `unsafe` sobre
  código nativo; ver [TODO](../plan/TODO.md).
- O detector de segredos do `pack` pega só padrões conhecidos.

## Não aplicável

- `pnpm audit`: o app não tem dependências JS em tempo de execução (UI sem bundler). O `jco`
  (npm) é usado só para compilar o mini-app de exemplo em JavaScript, com versão fixa.
- Python/C++: Fases 3 e 4 adiadas; sem código nessas camadas.
