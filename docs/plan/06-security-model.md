# 06 · Modelo de Segurança

Baseado em Zero Trust e capabilities ([ADR-006](04-adrs/ADR-006-capabilities-isolation.md)); lições de WeChat (MiniCPRF, segredos embutidos, supply chain), Alipay e FinClip.

## Ativos
Dados do usuário (FS, credenciais), integridade do host, canal IPC, pacotes de mini-apps, runtime Python opcional, atualizações.

## Ameaças (STRIDE resumido)

| Ameaça | Vetor | Controle |
|---|---|---|
| Spoofing | UI comprometida forja comandos IPC | Isolation Pattern, chaves efêmeras, validação de payload |
| Tampering | Pacote de mini-app/atualização adulterado | Assinatura + verificação de hash; update assinado |
| Repudiation | Ação sem rastro | Log de auditoria de comandos negados/permitidos |
| Information disclosure | Mini-app lê dados de outro; segredos no pacote | Memória linear Wasm, storage por mini-app, varredura de segredos no empacotamento |
| DoS | Loop infinito/estouro de memória | Fuel metering, limites de memória/tempo, quotas |
| Elevation | Escape do sandbox, path traversal, XSS | Capabilities com escopo, CSP restritiva, WIT mínimo, bloqueio de origens externas não permitidas |

## Controles por camada

- **WebView**: CSP restritiva; sem `eval`; origens externas bloqueadas por padrão; Isolation Pattern.
- **Core**: capabilities estáticas; comandos mínimos; validação de tamanho/formato em todo payload binário; sem portas localhost.
- **Wasm**: sem I/O por padrão; imports WIT apenas conforme manifesto aprovado; fuel + limite de memória + timeout; storage isolado por mini-app.
- **Python/C++**: código do host apenas (nunca de terceiros); falhas contidas (`Result`, worker isolado).
- **Cadeia de suprimentos**: `cargo-deny`/`cargo-audit`, `pnpm audit`, lockfiles, SBOM, builds reproduzíveis quando possível.

## Ciclo de vida de mini-app
1. Empacotar → manifesto (id, versão, permissões, hash) → assinar.
2. Instalar → verificar assinatura/hash → exibir permissões → consentimento do usuário.
3. Executar → imports WIT concedidos apenas conforme aprovado.
4. Atualizar → nova assinatura; mudança de permissões exige novo consentimento.
5. Revogar/remover → limpar storage; lista de revogação.

## Suíte de ataques (gate da Fase 2)
Loop infinito; alocação de memória ilimitada; leitura de FS fora do escopo; rede para host não declarado; chamada WIT não importada; manifesto adulterado; pacote com assinatura inválida; segredo embutido.

### Resultado da suíte de ataques (Fase 2, 2026-11-10)
Todos rodam em `cargo test` (e portanto na CI de Windows, Linux e macOS). "Contido" = erro claro ao usuário, host seguindo responsivo e outros mini-apps intactos.

| Ataque | Onde é testado | Resultado |
|---|---|---|
| Loop infinito | `attacks::infinite_loop_is_stopped_by_fuel`, `ui_attacks::limits_requested_in_the_manifest...` | Contido por fuel em < 2 s (`OutOfFuel`) |
| Guest só de CPU com fuel "infinito" | `attacks::cpu_only_guest_hits_timeout...` | Contido por timeout; executor de uma thread segue respondendo (≥ 10 ticks em 250 ms) |
| Alocação de memória ilimitada | `attacks::memory_hog...`, `ui_attacks::limits...` | Contido (`OutOfMemory { limit_bytes }`), com o limite do manifesto aplicado |
| FS e rede sem permissão | `attacks::fs_and_network_are_denied...` | Negados |
| Import WIT não concedido | `attacks::import_not_granted...`, `packaged::code_importing_more...` | Falha ao **carregar**; auditado como `DeniedNoCapability` |
| Aprovação maior que o manifesto | `packaged::approval_cannot_exceed...`, `lifecycle::approval_beyond...` | Recusada |
| Manifesto adulterado / assinatura inválida / chave não confiável / troca de chave | `superapp-package` (14 testes) | Recusados antes de qualquer execução |
| Wasm adulterado ou trocado | `package::tampered_wasm...`, `wasm_swapped...`, `packaged::tampered_real_package...` | `HashMismatch` |
| Tamanho declarado absurdo (4 GiB), bytes sobrando/faltando | `package::malformed_containers...` | Recusado sem alocar |
| Campo/permissão desconhecidos em manifesto assinado | `package::signed_manifest_with_unknown...` | Recusado |
| **Segredo embutido** | `package::embedded_secrets_are_refused_at_pack_time` | `pack` recusa (PEM, AWS, GitHub, Slack, Google, `sk-`) |
| Rollback de versão | `lifecycle::updates_must_increase_version...` | Recusado |
| Chave de autor revogada | `lifecycle::untrusted_or_tampered...` | App instalado passa a "com problema" e não executa |
| Pacote em disco adulterado após instalar | `lifecycle::untrusted_or_tampered...` | Detectado ao reabrir |
| Path traversal em chave de storage / id de app | `attacks::dir_storage_blocks_path_traversal...`, `lifecycle::uninstall...` | Contido (chave vira hex; id restrito) |
| Quota de storage | `attacks::storage_roundtrip...` | Recusada (valor > 1 MiB) |
| UI maliciosa: HTML/script, campo de estilo, saída > 64 KiB, aninhamento, não-JSON, caractere de controle | `ui_attacks::malicious_ui_output...` | Todas recusadas (`InvalidUi`); nada é desenhado |
| Falha (panic) do guest | `ui_attacks::panic_in_guest...` | Trap contido; host segue |
| Um app malicioso afetando outro | `ui_attacks::a_misbehaving_app...` | Sem efeito (instâncias e dados separados) |
| Vazamento de estado entre execuções | `ui_attacks::repeated_runs...` | Instância nova por execução |
| Rede por qualquer caminho (inclusive `fetch` embutido num interpretador) | `js_guest` + ausência de `wasi:http` no linker | O host nunca liga `wasi:http`/sockets: um componente que os importe nem carrega |
| Rede **mediada** ([ADR-011](04-adrs/ADR-011-miniapp-network.md)): domínio fora da lista, `http://`, IP literal, porta ≠ 443, usuário na URL, IP interno/loopback/metadata após o DNS, redirecionamento para fora, `Authorization`/`Cookie`, resposta > 1 MiB ou binária, 5ª chamada, taxa por minuto | `net` (9 unitários), `net.rs` (7 de ponta a ponta), `netbackend` (5) | Todas recusadas **antes** de sair qualquer byte, auditadas como `DeniedByNetworkRule`; conexão fixada no IP validado; app sem aprovação nem carrega |
| Exfiltração por mini-app com `storage` + `http` (limitação inerente) | consentimento + auditoria | O consentimento lista os domínios e avisa; só vão a esses domínios; cada chamada fica no log. Recomendação: app com dados pessoais não pede `http` |
| Mini-app sem capability no shell (janela `untrusted`) chamando `apps.*` | smoke (`apps: janela sem capability...`) | `permission_denied` |

**Limites conhecidos (não contidos nesta fase):** canais laterais de tempo (o relógio WASI existe para o guest); o detector de segredos pega só padrões conhecidos; sem revogação central de chaves nem auto-update (Fase 5).
