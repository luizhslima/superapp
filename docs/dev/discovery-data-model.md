# Mini-app `discovery`: modelo de dados (D0, schema 1)

Plano: [11-feature-startup-toolkit-discovery.md](../plan/11-feature-startup-toolkit-discovery.md).
Este documento fixa **o que se guarda e como**, antes de escrever o mini-app.

## Restrições da plataforma que moldam o modelo

| Fato | Consequência |
|---|---|
| `storage` só tem `get`, `set`, `delete` (sem listar chaves) | cada coleção tem uma **chave de índice** com a lista de ids |
| chave ≤ 100 B, valor ≤ 1 MiB, total ≤ 4 MiB por app | um registro por chave; limites de quantidade e de texto abaixo |
| cada evento roda numa instância nova | todo o estado (inclusive a tela atual) vai para o storage |
| gravações em várias chaves **não são atômicas** | ordem fixa de escrita e reparo na abertura (ver "Consistência") |
| `values` de um clique ≤ 16 000 caracteres no total; campo longo ≤ 4000 | uma ficha por tela, salva num clique só |
| o backup por app exporta todas as chaves em JSON | nada fica fora do storage; restaurar = estado completo |

## Chaves

| Chave | Conteúdo |
|---|---|
| `meta` | `{"schema":1,"seq":N}`: versão do esquema e contador de ids |
| `ui` | `{"tela":"entrevistas","sel":"i7","filtro":"h2"}`: tela atual, item aberto e filtro |
| `idx:t` `idx:h` `idx:i` `idx:v` `idx:d` | lista de ids de cada coleção, em ordem de criação (`["h1","h4"]`) |
| `t1`, `t2`... | território (a chave é o próprio id) |
| `h3`... | hipótese |
| `i4`... | entrevista (ficha) |
| `v5`... | evidência (D3) |
| `d6`... | decisão (D3) |

Ids: prefixo da coleção + número de `meta.seq` (`h1`, `i7`, ...). Nunca reaproveitados.
`em`: milissegundos Unix (permissão `clock`), data de criação. Papel e canal são guardados como códigos (`executor`, `indicacao`...) e mostrados com rótulos em português.

## Registros

Todos os campos de texto são limitados em **caracteres**; o app recorta/recusa antes de gravar.

**Território** `t<n>` (máx. 5): `nome` (≤ 80), `processo`, `executor`, `responsavel`,
`contexto`, `alternativa`, `gatilho` (cada um ≤ 300), `em`.

**Hipótese** `h<n>` (máx. **20**: cada uma vira uma caixa de marcação na ficha de entrevista e a tela aceita 32 campos): `t` (id do território), `texto` (≤ 500), `procura` (evidência
procurada, ≤ 500), `contra` (evidência contrária, ≤ 500), `experimento` (≤ 500), `status`
(`aberta` | `confirmada` | `refutada` | `reformulada`), `em`.

**Entrevista** `i<n>` (máx. 200): `h` (ids de hipóteses testadas), `participante` (**código**,
ex. `P3`; ver Privacidade), `empresa` (≤ 80, opcional), `papel` (`executor` | `responsavel` |
`comprador` | `outro`), `canal` (`indicacao` | `comunidade` | `email` | `evento` | `outro`),
`data` (texto `AAAA-MM-DD`, opcional), `fato`, `interpretacao`, `duvida` (cada ≤ 4000), `observacao` (houve observação
autorizada: bool), `em`.

**Evidência** `v<n>` (máx. 500): `h` (id da hipótese), `fonte` (id de entrevista, como `i3`, ou texto/URL ≤ 300),
`resumo` (≤ 500), `forca` (1 a 7), `contra` (bool: evidência **contrária**), `empresa` (≤ 80,
herdada da entrevista quando a fonte é uma), `papel` (herdado da mesma entrevista), `em`.

Escala de força (do doc de pesquisa): 1 reclamação pública · 2 entrevista detalhada ·
3 observação autorizada · 4 tentativa anterior de solução · 5 acesso ao responsável ·
6 piloto com escopo · 7 pagamento.

**Decisão** `d<n>` (máx. 100): `t` (território), `h` (hipóteses consideradas), `escolha`
(`avancar` | `restringir` | `reformular` | `abandonar`), `porque` (≤ 1000), `proximo` (próximo
experimento, ≤ 500), `em`.

## O que o painel calcula (nada disso é gravado)

- **Empresas independentes** por hipótese: empresas distintas (texto normalizado: minúsculas, sem
  espaços nas pontas) entre as evidências a favor. Volume de falas e curtidas **não** entram.
- **Diversidade de papéis**: quantos papéis diferentes aparecem.
- **Maior força** de evidência a favor e contra, por hipótese.
- **Pendências**: hipótese sem evidência contrária buscada (`contra` vazio), entrevistas sem
  `duvida`/`interpretacao`, contradições abertas (evidência contrária sem decisão posterior).
- **Critérios de avanço** do doc de pesquisa, como lista de marcações (segmento, episódios em
  empresas independentes, consequências, alternativas, mobilização, acesso): sugestão, não regra.

## Orçamento de espaço

Pior caso por entrevista: 3 × 4000 caracteres (até ~12 KB em pt-BR, com acentos de 2 bytes ≈ 24 KB).
Uso típico (~1 KB): 200 entrevistas ≈ 0,2 MiB. O app lê o espaço usado a cada gravação e,
quando `set` devolve erro de quota, mostra o erro e **não perde** o que foi digitado (a tela volta
com os valores). Aviso proativo previsto quando o uso estimado passar de 3 MiB.

## Consistência

- Escrita de um registro novo: (1) `meta.seq`, (2) registro, (3) `idx:x`. Uma queda no meio deixa, no
  pior caso, um registro **órfão** (existe, mas fora do índice) ou um id queimado, nunca um
  índice apontando para o vazio.
- Remoção: (1) `idx:x`, (2) registro. Regras (todas com confirmação): remover um **território** só se não houver
  hipóteses nele; remover uma **hipótese** só se não houver evidências ligadas (as entrevistas
  apenas perdem a ligação); remover uma **entrevista** apaga também as evidências que a citam como
  fonte (pedido de exclusão: nada fica).
- **Reparo na abertura** (`Init`): remove do índice ids sem registro e reinclui órfãos já
  conhecidos pelo prefixo da sequência (`1..=meta.seq`). Barato porque a sequência é curta.

## Versão e migração

- `meta.schema` é a versão. Na abertura: igual → segue; **menor** → migra em ordem (cada migração
  é uma função pura testada com fixtures) e grava `schema` por último; **maior** (app antigo com
  backup novo) → só leitura, com aviso.
- Mudança compatível (campo novo opcional) não exige subir `schema`; leitor ignora o que não
  conhece e usa padrão para o que falta.
- Backup/restauração do host já valida chave, tamanho e quota; o app valida o conteúdo ao abrir.

## Privacidade

Há dados pessoais de terceiros em entrevistas (LGPD).

- O modelo guarda **código do participante** (`P3`), não nome; empresa é opcional. A tela orienta a
  manter a tabela de correspondência fora do app.
- Nada sai da máquina: o mini-app não tem rede. Exportar é ação explícita do usuário (backup JSON
  do host ou Markdown, em D4).
- Pedido de exclusão: remover a entrevista apaga a ficha e as evidências que a citam como fonte
  (com confirmação), e o backup antigo continua sendo do usuário.

## Fora de escopo do schema 1

Transcrições completas (ficam em arquivo fora do app), anexos, múltiplos usuários/sincronização,
templates de roteiro editáveis (o roteiro padrão é texto fixo no app) e fases 2 a 4.
