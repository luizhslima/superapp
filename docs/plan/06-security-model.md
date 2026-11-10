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
