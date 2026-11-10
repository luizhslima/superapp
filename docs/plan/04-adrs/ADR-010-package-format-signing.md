# ADR-010 · Pacote `.sapp`, manifesto e assinatura

- **Status**: Accepted (2026-11-10) — implementado em `crates/package`
- **Contexto**: mini-apps de terceiros precisam de um formato de distribuição cuja integridade e autoria o host verifique antes de executar qualquer byte (R7, modelo de ameaças "Tampering"). Custo zero: sem serviço de assinatura pago.
- **Decisão**:
  - **Container próprio e mínimo**, não zip: `"SAPP" | versão | chave pública (32) | assinatura (64) | len_manifesto | len_wasm | manifesto JSON | wasm`. Sem diretórios, sem compressão, sem nomes de arquivo: elimina path traversal, zip bombs e ambiguidade de parsers. Tamanhos são checados contra tetos (manifesto 64 KiB, wasm 32 MiB) e contra o tamanho real do arquivo **antes** de alocar ou fatiar.
  - **Ed25519** (`ed25519-dalek`, `verify_strict`, que rejeita assinaturas maleáveis e chaves de ordem pequena) sobre `"superapp-sapp-v1\0" || bytes exatos do manifesto`. O prefixo é separação de domínio. Assinar os bytes exatos evita canonicalização de JSON.
  - **O manifesto carrega o SHA-256 do wasm**: uma única assinatura cobre manifesto e código.
  - **Confiança por lista de chaves** (`TrustStore`): o pacote traz a chave pública do autor, mas ela só vale se estiver na lista do host. Uma assinatura válida de chave desconhecida é recusada.
  - **Ordem de verificação**: estrutura → signatário confiável → assinatura → manifesto → hash.
  - **Manifesto estrito** (`deny_unknown_fields`): campo ou permissão desconhecidos invalidam o pacote (ignorar em silêncio seria perigoso entre versões). Valida id (`[a-z0-9-]{1,32}`), nome, versão `N.N.N`, permissões sem repetição, hash e tetos de limites (fuel 500 M, memória 64 MB, timeout 30 s).
  - **Permissão efetiva = aprovada ∩ declarada**: `load_verified` recusa aprovação maior que a pedida; só o aprovado é ligado ao linker. Código que importe mais do que o manifesto declara falha ao carregar, mesmo assinado (o usuário nunca viu essa permissão).
- **Consequências**: (+) verificação barata, sem dependências pesadas, superfície de parser mínima, fácil de testar com fuzz/adulteração. (−) formato próprio (sem ferramentas prontas; há `cargo xtask keygen|pack|verify`); revogação e rotação de chaves não estão resolvidas.
- **Não coberto (ficar para a entrega 3 e Fase 5)**: **anti-rollback** (recusar versão menor que a instalada), atualização com mudança de permissões exigindo novo consentimento (E2.4), **revogação de chaves**, distribuição/registro e auto-update (Fase 5). Chaves privadas de desenvolvimento (`*.key`) nunca vão para o git.
- **Riscos**: R7, R10.
