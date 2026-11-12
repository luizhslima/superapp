# Feature: ferramentas de apoio à criação de startups (fase 1: Discovery)

**Status:** planejamento (2026-11-12). Nada implementado.
**Pesquisa de base:** [docs/research/](../research/)
([Customer Discovery B2B e stack open source](../research/customer-discovery-b2b-e-stack-opensource.md),
[literatura sobre IA e ciclo de validação](../research/literatura-ia-e-ciclo-de-validacao.md)).

## Panorama

Objetivo de longo prazo: um conjunto de ferramentas que **apoia as decisões** de quem cria uma
startup, da descoberta até a entrega e os incrementos dos ciclos. Cada fase vira uma ferramenta
(mini-app) sobre a mesma base de **evidências e decisões**:

| Fase | Pergunta central | Estado |
|---|---|---|
| 1. Discovery (problema) | Existe um problema relevante e mal resolvido, para quem? | **esta feature** |
| 2. Validação da solução | Uma intervenção melhora o trabalho? | futuro |
| 3. Validação comercial | Existe compra viável? | futuro |
| 4. Entrega e ciclos | O que construir/medir no próximo incremento? | futuro |

Ideia que atravessa todas: **evidência com força e fonte → decisão registrada (avançar, restringir,
pivotar, abandonar)**. O que se constrói na fase 1 (modelo de dados e painel de evidência) é
reaproveitado nas seguintes.

## Decisões já tomadas

- **Forma:** mini-app Wasm do Superapp (ADR-004/009), não funcionalidade nativa do host. Usa a
  plataforma com um caso real e mantém o isolamento.
- **Primeira fatia:** duas trilhas em paralelo: **A. bancada manual** (implementar) e
  **B. automação de escuta** (só desenho e spike; nada em produção).
- **Princípio da pesquisa:** *conversar antes de montar máquina de prospecção*. Automação organiza
  e encontra candidatos; não valida nada sozinha. Agente de coleta nunca envia mensagens.

## Trilha A: bancada manual de Discovery

Mini-app `discovery` (id), só com permissão `storage` (e `log`). Telas, cada uma uma árvore
declarativa; o estado vive no storage (cada evento roda numa instância nova).

1. **Território**: processo, executor, responsável, contexto, alternativa atual, gatilho
   (a tabela do doc de pesquisa). Pode haver vários territórios.
2. **Hipóteses**: enunciado, evidência procurada, evidência contrária, experimento, status
   (aberta / confirmada / refutada / reformulada).
3. **Entrevistas**: roteiro padrão (abertura, episódio, dificuldades, alternativas, fechamento) e
   **ficha por entrevista** separando *fato*, *interpretação* e *dúvida*; empresa, papel
   (executor/responsável/comprador), canal de origem, data.
4. **Evidências**: cada item liga fonte/entrevista e hipótese e tem **força** (escala do doc:
   reclamação pública < entrevista < observação < tentativa anterior de solução < acesso ao
   responsável < piloto < pagamento).
5. **Painel e decisão**: contagem de **empresas independentes** (não de falas), diversidade de
   papéis, hipóteses sem evidência contrária buscada, contradições abertas; critérios de avanço
   do doc (segmento, episódios, consequências, alternativas, mobilização, acesso). Registro da
   decisão com data e justificativa.
6. **Exportar**: o backup JSON por app já existe; acrescentar exportação em Markdown legível
   (ficha, painel) quando o formato estabilizar.

### O que a plataforma ainda não cobre (lacunas)

| Lacuna | Por quê importa | Encaminhamento |
|---|---|---|
| Campo de texto **multilinha** | notas de entrevista passam de uma linha (hoje `input` = 1 linha, ≤ 1000 caracteres) | novo nó `textarea` na UI declarativa (extensão do ADR-009; mesmos limites e validação) |
| Quota de storage 4 MiB, valor ≤ 1 MiB | suficiente para dezenas de entrevistas em texto; transcrições longas não | monitorar; transcrições ficam fora (link/arquivo) por ora |
| Orçamento de 200 nós / 64 KiB por tela | listas grandes de evidências | paginar/filtrar por hipótese |
| Sem navegação nativa | várias telas | `select`/botões + estado no storage (já funciona, como no `tarefas`) |
| Sem seleção por data, ordenação | filas por data | `now_ms` (permissão `clock`) e ordenação no próprio app |

### Entregas da trilha A

- **D0** Modelo de dados + esquema de chaves do storage, versionado (`schema: 1`), com migração
  prevista. Doc curto em `docs/dev/`.
- **D1** `textarea` na UI declarativa (crate `ui`, renderer do shell, SDK, testes, fuzz).
- **D2** Mini-app `discovery`: Território, Hipóteses, Entrevistas (testes de ponta a ponta em Wasm,
  como `tarefas`).
- **D3** Evidências, painel e decisão.
- **D4** Exportação Markdown; uso real por duas semanas e revisão.

Critério de pronto da fatia: conduzir uma rodada real de entrevistas só com o mini-app, sem
planilha paralela.

## Trilha B: automação de escuta (desenho + spike)

Hoje o host **não dá rede nem IA** a mini-apps (permissões: `log`, `storage`, `clock`; o host
nunca liga `wasi:http`), e mini-apps só rodam em resposta a um clique (sem agendamento). Qualquer
automação exige decisões de segurança novas, por isso é desenho primeiro.

Pipeline-alvo (do doc de pesquisa): sinais → normalizar → deduplicar → filtrar ruído → extrair
episódio por LLM com schema → **revisão humana** → candidatos a entrevista. Convite sempre manual.

Questões de desenho (viram ADR):

1. **Rede**: capability `http` mediada pelo host, com **lista de domínios** declarada no manifesto
   e aprovada no consentimento, limites de tamanho/tempo/taxa e auditoria. Resposta marcada como
   **dado não confiável** (nunca vira instrução).
2. **LLM**: interface do host (chave do usuário em `G:\dev\secrets`/cofre local, nunca no mini-app),
   saída por schema, tamanho e custo limitados, e aviso do que sai da máquina (LGPD: o doc de
   pesquisa pede revisar retenção e subprocessadores).
3. **Execução periódica**: como rodar sem clique (agendador do host que dispara um evento
   `Tick`, com teto de frequência) **ou** coletor externo que só deposita um arquivo de entrada
   (inbox) que o mini-app importa. O segundo não exige novo poder do mini-app.
4. **Fontes e termos**: Reddit, LinkedIn e WhatsApp têm regras próprias. Regra do projeto:
   automatizar pesquisa e registro; **interação em plataformas fica manual** ou por meios oficiais.
5. **Defesa contra injeção**: conteúdo de fóruns nunca chega a um agente com capacidade de agir.

Candidatos a avaliar no spike (listados na pesquisa; nada adotado): Reddit Complaint Miner,
SearXNG, Crawl4AI, Activepieces/n8n, Formbricks, LangGraph. Spike descartável, fora do workspace:
um coletor mínimo que gera um `inbox.json`, importado pelo mini-app, para medir o ganho real antes
de dar rede ao host.

### Entregas da trilha B

- **B0** ADR "capacidades de rede/IA e execução periódica para mini-apps" (Proposed).
- **B1** Spike do coletor com **inbox de arquivo** (sem mexer no host) + relatório curto.
- **B2** Só se B1 provar valor: implementar a capability escolhida no ADR, com testes de negação
  (política, domínios, tamanho) e smoke.

## Ordem sugerida

1. D0 (modelo) e B0 (ADR) em paralelo, são só documentos.
2. D1 → D2 → D3.
3. B1 entra quando a bancada já estiver sendo usada (assim o spike resolve uma dor real).
4. D4 e revisão; só então decidir B2 e a fase 2 (validação da solução).

## Riscos

- **Virar ferramenta sem uso**: construir antes de entrevistar. Mitigação: critério de pronto é uma
  rodada real; escopo da bancada fica pequeno.
- **Falsa validação por contagem**: o painel mede empresas independentes e força da evidência, não
  volume de falas ou curtidas.
- **Dados pessoais de entrevistados** no storage: manter só o necessário, orientar anonimização,
  lembrar do backup (já exporta tudo em JSON legível) e do direito de exclusão.
- **Plataforma pequena demais** (textarea, quota): resolver com extensões pequenas e testadas, sem
  abrir exceções de segurança.
