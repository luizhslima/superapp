# Microsserviços do superapp (casca)

Cada linguagem foi escolhida pelo que resolve melhor:

| Serviço | Linguagem | Porta | Por quê |
|---|---|---|---|
| `ai-gateway` | Node + TypeScript (Fastify) | 8081 | Só espera rede (LLMs): I/O concorrente, streaming e proxy fino. Único que conhece a chave de API. |
| `rag` | Python (FastAPI) | 8082 | Embeddings, chunking e busca vetorial: o ecossistema de IA/dados é o mais maduro. |
| `events-java` | Java 21 (Spring Boot) | 8083 | Backbone de eventos: cliente Kafka maduro, tipagem forte, ordem por chave e entrega idempotente. |
| `kafka` | apache/kafka (KRaft) | 29092 | Barramento de eventos, sem Zookeeper. |

Fluxo previsto: `rag` -> `ai-gateway` -> provedor de LLM; eventos de domínio passam por `events-java` (Kafka).

## Rodar

```
cd services
cp .env.example .env        # opcional; o padrão é LLM_PROVIDER=echo (sem custo, sem rede)
docker compose up --build
```

Tudo escuta só em `127.0.0.1`.

Testar:

```
curl -s localhost:8081/v1/complete -H 'content-type: application/json' -d '{"prompt":"oi"}'
curl -s localhost:8082/v1/ingest   -H 'content-type: application/json' -d '{"doc_id":"a","text":"fechamento financeiro leva dias"}'
curl -s localhost:8082/v1/query    -H 'content-type: application/json' -d '{"question":"fechamento","answer":true}'
curl -s localhost:8083/v1/events   -H 'content-type: application/json' -d '{"type":"order.created","key":"o1","payload":"{}"}'
curl -s localhost:8083/v1/events/recent
```

## Testes sem Docker

- `ai-gateway`: `npm install && npm test`
- `rag`: `pip install -r requirements.txt && pytest`
- `events-java`: `mvn test` (ou via Docker: `docker build ./events-java`)

## O que é casca (e o que falta)

- `rag`: o "embedding" é um hash de palavras, **não semântico**, e o armazém é em memória. Trocar por modelo real e pgvector/Qdrant.
- `ai-gateway`: só `echo` e `anthropic` (sem streaming ainda). Faltam limite de taxa por cliente e autenticação entre serviços.
- `events-java`: texto simples `tipo|payload`, consumidor guarda em memória. Falta contrato (JSON Schema/Avro), DLQ e retentativas.
- Sem autenticação entre serviços: por isso as portas ficam em `127.0.0.1`. Antes de expor, adicionar.
- Auto-criação de tópicos do Kafka está ligada só para dev.

## Regras que os serviços já seguem

- Entradas com limites explícitos; erros de provedor não vazam detalhe nem chave.
- Trechos recuperados no `rag` são **dados não confiáveis**: vão delimitados no prompt, nunca como instrução.
- Chave de API só no `ai-gateway`, via ambiente (`.env` não é versionado).
- Containers rodam como usuário sem privilégio.
