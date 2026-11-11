# Publicar uma versão e atualizar o app

Custo zero: tudo roda no GitHub Actions e nos Releases do repositório.

## Chaves

- O atualizador usa uma chave **minisign** própria (não é certificado do SO). A pública está em
  `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`) e em
  [../security/updater-pubkey.txt](../security/updater-pubkey.txt).
- A **privada e a senha** ficam fora do repositório (`G:\dev\secrets` na máquina do mantenedor) e
  nos segredos do GitHub: `TAURI_SIGNING_PRIVATE_KEY` (conteúdo do arquivo `.key`, sem alterar) e
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- **Perder a chave ou a senha impede qualquer atualização dos apps já instalados** (eles só aceitam
  pacotes assinados por ela). Mantenha cópia fora do PC.
- Trocar de chave exige uma versão nova assinada pela chave antiga que já embuta a pública nova.

## Passo a passo

1. Subir a versão em `Cargo.toml` (`workspace.package.version`) e em `src-tauri/tauri.conf.json`.
2. `git commit`, `git push`, e `git tag vX.Y.Z && git push origin vX.Y.Z`.
3. O workflow `release` constrói nos 3 sistemas e cria um **rascunho** de Release com instaladores,
   assinaturas (`.sig`) e `latest.json`.
4. Conferir o rascunho e **publicá-lo**. Só depois de publicado o endereço
   `releases/latest/download/latest.json` passa a servir a versão nova.
5. Apps antigos: botão "Verificar atualizações" na tela principal.

## O que o app confere

- O endereço do `latest.json` é fixo no config; a UI não informa URL.
- HTTPS obrigatório; assinatura minisign verificada contra a chave embutida antes de instalar.
- Chamadas passam pelo Router (capability `app:update`, só janela `main`) e são auditadas.

## Limites conhecidos

- Sem assinatura de código do SO: Windows (SmartScreen) e macOS (Gatekeeper) avisam na instalação.
- Sem canal de teste/beta: o `latest` é o único canal.
- Sem proteção contra downgrade além da comparação de versão feita pelo plugin.
