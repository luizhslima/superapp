# ADR-011 · Rede para mini-apps: permissão `http` mediada pelo host

- **Status**: Accepted (2026-11-13) — **v1 implementado** (permissão `http`, política, backend `reqwest`, consentimento, testes). Ficam para depois: cofre de segredos, interface `llm`, execução periódica, taxa por domínio e smoke contra servidor local (ver "Estado da implementação")
- **Contexto**: hoje um mini-app só tem `log`, `storage` e `clock`; o host nunca liga `wasi:http`. Casos reais (a trilha B do [Discovery](../11-feature-startup-toolkit-discovery.md): coletar páginas e APIs, depois LLM) precisam de internet. Dar rede **crua** quebraria o modelo de segurança: um app aprovado com `storage` + rede vira canal de exfiltração, e o código dele poderia alcançar a rede local do usuário.
- **Decisão**: nova permissão **`http`**, com a requisição **feita pelo host** (Rust), nunca pelo guest.
  - **Interface WIT** `superapp:host/http`: `fetch(request) -> result<response, string>`, uma chamada síncrona do ponto de vista do app. Como as demais interfaces, importar sem concessão faz a instanciação falhar.
  - **Declaração e consentimento**: o manifesto ganha `network: { "hosts": ["api.exemplo.com", "*.reddit.com"] }`, obrigatório e não vazio quando há `http`; `hosts` sem `http` invalida o pacote. O consentimento mostra os domínios e avisa que o app pode enviar para eles qualquer dado que lê. A permissão efetiva continua sendo **aprovada ∩ declarada**; a lista de domínios aprovada é a do manifesto (sem edição parcial no v1).
  - **Regras por requisição** (todas no host, todas com teste de negação):
    - só **`https`**; métodos `GET` e `POST`; porta padrão; host exato ou curinga de um nível (`*.x.com`), nunca IP literal;
    - **cada redirecionamento é revalidado** contra a lista (máx. 3) e contra as regras de IP;
    - **IPs bloqueados após resolver o DNS**: loopback, privados (RFC 1918), link-local e metadata (`169.254.0.0/16`), CGNAT, ULA/link-local IPv6; a conexão usa o IP validado (evita *DNS rebinding*);
    - corpo de envio ≤ 64 KiB; resposta ≤ 1 MiB (cortada com erro, nunca truncada em silêncio); só tipos de texto/JSON; timeout de 10 s por chamada, no máx. **4 chamadas por execução** e teto de 30 s no total (soma com o limite de execução do app);
    - cabeçalhos: o host define `User-Agent`; o app só pode `Accept` e `Content-Type`; sem cookies, sem credenciais ambientes, sem `Referer`;
    - taxa: teto por app e por domínio (janela deslizante) para não virar ferramenta de abuso.
  - **Resposta é dado não confiável**: nunca vira instrução nem entra na UI como HTML (a UI declarativa já só usa `textContent`). Páginas e fóruns coletados só alimentam o app; **agentes de coleta não enviam mensagens**.
  - **Auditoria**: cada chamada registra `app:<id>`, host, método, status e bytes de ida e volta (sem corpo, sem URL com query); negações por falta de permissão entram como `DeniedNoCapability` e as de regra de rede (domínio, IP, limite) como novo motivo `DeniedByNetworkRule` em `Decision`.
  - **Só dentro de uma execução iniciada pelo usuário**: não há rede em segundo plano. Execução periódica fica **fora deste ADR** (ver abaixo).
  - **Segredos (chaves de API)**: o app **nunca** vê a chave. Fica para o passo seguinte um *cofre* do host, em que o usuário cadastra um segredo por app e o app referencia só o nome (`secret:nome`) num cabeçalho `Authorization` que o host preenche. Até lá, só APIs sem chave.
  - **LLM**: não é rede direta. Vira interface própria (`llm`) depois deste ADR, sobre o mesmo cliente, com chave no cofre, saída por schema e limite de tamanho/custo.
- **Separação recomendada de dados sensíveis**: mini-app que guarda dados pessoais (ex.: fichas de entrevista do `discovery`) **não pede `http`**. O coletor é **outro** mini-app e entrega o resultado por arquivo de importação. Assim nenhum app combina dados sensíveis com saída de rede. O consentimento destaca a combinação `storage` + `http`.
- **Implementação**: o host já tem `reqwest` (Loja); a política (domínios, IPs, limites) é uma função pura em `crates/` com testes e fuzz próprios, e o `wasm-host` só chama essa função. Em `crates/package`: nova `Permission::Http` e o campo `network` (manifesto continua `deny_unknown_fields`). `xtask`: `smoke` ganha chamadas negadas (domínio fora da lista, IP privado, redirecionamento para fora, resposta grande, 5ª chamada) contra um servidor local de teste.
- **Consequências**: (+) internet para mini-apps sem abrir mão do isolamento; auditável; usuário vê para onde o app pode falar. (−) um app aprovado pode enviar os dados que lê **aos domínios listados** (limitação inerente; reduzida por escopo, auditoria e a separação acima); mais superfície no host (DNS, redirects, TLS) a manter; manifesto e consentimento mudam (versão do esquema do pacote).
- **Execução periódica (decisão adiada)**: duas opções, em ordem de preferência: (1) **coletor externo + arquivo de entrada** que o app importa por clique (nenhum poder novo); (2) agendador do host disparando um evento `Tick` com teto de frequência, só para apps aprovados para isso. Decidir após o spike B1 do Discovery.
- **Alternativas descartadas**: `wasi:http` direto (rede crua, sem lista, sem auditoria nem regras de IP); proxy local aberto (qualquer processo da máquina usaria); permitir rede só à webview (o código do guest não participaria; e abriria o caminho do ADR-012 sem decisão).
- **Riscos**: R7 (exfiltração, SSRF, abuso: estender o item no [modelo de segurança](../06-security-model.md)) e R8 (escopo: manter o v1 mínimo, sem cofre nem `llm`). Revisar antes de liberar o consentimento de `http` no catálogo.

## Estado da implementação (2026-11-13)

Feito no v1:
- `crates/package`: `Permission::Http` e `network.hosts` (manifesto estrito: `http` exige a lista, a lista só vale com `http`; 1 a 8 domínios, minúsculos, nunca IP nem nome de rótulo único; sem repetição). Pacotes sem rede não mudam de bytes (o campo nem é escrito), então as assinaturas existentes seguem válidas.
- `crates/wasm-host/src/net.rs`: política pura (URL, domínio exato ou `*.dominio` de um nível, IPs públicos, redirecionamentos revalidados, cabeçalhos, limites de corpo, tipos de conteúdo, limites por execução e por minuto) e o *trait* `NetBackend` (DNS + transporte), com 9 testes unitários que usam um backend falso.
- Interface WIT `superapp:host/http` (`fetch`), ligada ao componente só quando `http` foi aprovada; sem aprovação, o import falha ao carregar e vira `DeniedNoCapability`. Auditoria de cada chamada (`http.fetch MÉTODO host -> status sent= recv=`, sem query) e `Decision::DeniedByNetworkRule` para recusas por regra.
- `src-tauri/src/netbackend.rs`: transporte real com `reqwest`: conexão **fixada no IP validado**, sem seguir redirecionamentos, sem proxy do sistema, sem cookies, leitura em pedaços com teto, erros curtos sem a URL. Testado com um servidor local (IP fixado, redirecionamento devolvido, teto, timeout).
- Consentimento: a tela mostra os domínios e o aviso de que o app pode enviar a eles o que lê; uma **atualização que amplia `network.hosts`** conta como permissão nova (`added` inclui `http`).
- SDK Rust: `superapp_sdk::http::{fetch, get, post}`. Testes de ponta a ponta no Wasm (`crates/wasm-host/tests/net.rs`, guest em `tests/guests/net`).

Diferenças em relação ao texto da decisão:
- A taxa é **por app** (30 chamadas por minuto, compartilhada entre execuções), ainda sem janela por domínio.
- O teto de 30 s no total não foi criado à parte: vale o limite de execução do app (`limits.timeout_ms`, até 30 s); quem usa `http` deve pedir um tempo maior que o padrão de 5 s.
- Os testes de negação usam backend falso; **não há** smoke da janela real contra um servidor local de teste (os IPs internos seriam barrados de propósito).

Fica para os próximos passos: cofre de segredos (`secret:nome` em `Authorization`), interface `llm`, execução periódica (inbox de arquivo primeiro) e a taxa por domínio.

