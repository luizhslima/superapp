# 09 · Fluxo de Trabalho: Dev Solo + IA

## Princípios
- **Tarefas pequenas e verificáveis**: cada tarefa cabe num PR curto com critério de aceite de [05](05-backlog-epics.md).
- **Humano decide arquitetura; IA implementa e revisa**: decisões viram ADR antes do código.
- **Spike antes de feature**: peça à IA protótipos descartáveis em `spikes/`, com relatório objetivo.
- **Testes primeiro** nas fronteiras de segurança (capabilities, manifesto, sandbox).

## Ciclo por tarefa
1. Escolher história do backlog → reescrever CA em testes.
2. Planejar com a IA (plan mode) → revisar.
3. Implementar em branch → CI nos 3 SOs.
4. Revisão (code-review da IA + leitura sua) com foco em `unsafe`, fronteiras IPC/WIT e segurança.
5. Atualizar docs/ADR se contratos mudaram → merge.

## Checkpoints de revisão humana obrigatória
Qualquer `unsafe`; mudança de capabilities/CSP; esquema do manifesto; WIT; código de assinatura/update; dependências novas.

## Higiene
- Commits pequenos e frequentes; um assunto por PR.
- Não aceitar afirmações de performance sem benchmark.
- Revalidar números da investigação ([00](00-vision-scope.md)) com medições reais.
