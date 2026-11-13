# Escrevendo mini-apps para o Superapp

Um mini-app é um **componente WebAssembly** que roda dentro do host, isolado, com só as
permissões que o usuário aprovar. Este guia cobre o caminho feliz em Rust. O contrato é o WIT
em [`crates/wasm-host/wit/host.wit`](../../crates/wasm-host/wit/host.wit); qualquer linguagem
que gere componentes para `wasm32-wasip2` serve.

## Modelo mental

- O host chama o seu `run` **uma vez por evento**. Cada chamada usa uma instância nova: a
  memória do seu código **não** persiste. O que precisa durar vai para `storage`.
- Você não desenha HTML. Você devolve uma **árvore de UI** de um catálogo fixo; o host valida e
  desenha ([ADR-009](../plan/04-adrs/ADR-009-miniapp-ui.md)).
- Você só enxerga o que o manifesto pede **e** o usuário aprovou. Sem rede, sem arquivos, sem
  relógio, a menos que a permissão exista e seja concedida.

## Começando rápido: `new-app`

```bash
cargo xtask new-app minhas-notas "Minhas notas"      # cria miniapps/minhas-notas a partir de `tarefas`
cargo xtask build-app minhas-notas G:/dev/secrets/superapp-publisher.key   # compila e assina
```

O resultado é `miniapps/minhas-notas/minhas-notas.sapp`: arraste para o Superapp ou use
**Instalar de um arquivo .sapp** na aba Mini-apps. O gabarito é uma cópia de
[`miniapps/tarefas`](../../miniapps/tarefas), que usa campo de texto, linha, lista com ações e
progresso e é testado de ponta a ponta. Cada mini-app aberto ganha a própria janela do sistema (no macOS, abas nativas); a janela só fala com o próprio app. Só funciona se a chave que assinou estiver em
**Configurações → Autores confiáveis** (reinicie depois de adicionar).

## Começando (Rust)

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
superapp-sdk = { path = "../sdk" }   # miniapps/sdk neste repositório

[profile.release]
opt-level = "s"
lto = true
strip = true
```

```rust
use superapp_sdk::{export_app, storage, ui, App, Event, Node};

struct Contador;

impl App for Contador {
    fn handle(event: Event) -> Result<Node, String> {
        let mut n = storage::get_u64("n").unwrap_or(0);
        if matches!(&event, Event::Click { id, .. } if id == "mais") {
            n += 1;
            storage::set_u64("n", n)?;
        }
        Ok(ui::stack(vec![
            ui::text(format!("Valor: {n}")),
            ui::button("mais", "+1"),
        ]))
    }
}

export_app!(Contador);
```

Compile: `cargo build --release --target wasm32-wasip2` (uma vez: `rustup target add wasm32-wasip2`).
Exemplos completos: [`miniapps/clicker`](../../miniapps/clicker) (SDK) e
[`miniapps/counter`](../../miniapps/counter) (WIT direto, sem SDK).

## Outras linguagens (exemplo: JavaScript)

O contrato é o WIT, então qualquer linguagem com ferramenta de componentes serve.
[`miniapps/clicker-js`](../../miniapps/clicker-js) é o mesmo `clicker` em JavaScript, compilado
com [`jco componentize`](https://github.com/bytecodealliance/jco) (versão fixa em `build.mjs`):

```bash
node miniapps/clicker-js/build.mjs   # gera clicker-js.wasm (~11,5 MB; precisa de Node 20+)
```

Três pontos que só aparecem ao sair do Rust:

1. **Declare um mundo WIT enxuto.** O `jco` não remove importações não usadas: com o mundo
   completo do host o componente exigiria `log`, `storage` e `clock` mesmo usando só uma, e o
   usuário veria três permissões. O `build.mjs` gera um mundo `app` que importa só
   `superapp:host/storage`. Faça o mesmo com o que o seu app usa. (No SDK Rust isso é
   automático: o linker remove o código morto.)
2. **Desabilite `http`/`fetch-event`.** O interpretador embute `fetch`, que importa `wasi:http`.
   O host **nunca** liga `wasi:http`, então o componente nem carregaria; `--disable http
   fetch-event` o remove.
3. **Custo.** O interpretador (SpiderMonkey) vai dentro do componente: ~11,5 MB e ~8,5 s para
   compilar a primeira vez (feito na **instalação**, não no primeiro clique). Depois, cada
   execução leva poucos ms e ~10 M de fuel, dentro dos limites padrão.

## Eventos e UI

| Entrada (`Event`) | Quando |
|---|---|
| `Init` | o usuário abre o mini-app |
| `Click { id, values }` | o usuário aciona um botão (ou a ação de um item de lista) com esse `id`; `values` traz o valor atual de **todos os campos da tela** |

Os campos funcionam como um **formulário**: o que o usuário digita fica na tela e só viaja ao
mini-app quando ele aciona um botão. `values` mapeia o `id` do campo ao valor em texto (caixa:
`"true"`/`"false"`; seleção: a opção escolhida). Em Rust: `values.get("nova")`. Como cada
evento recria a tela, devolva no `value`/`checked`/`selected` o que quiser manter preenchido; o
que não for devolvido volta ao padrão. Enter num campo de texto aciona o botão da mesma linha.

| Componente (`ui::`) | Limites |
|---|---|
| `text(s)` | até 500 caracteres, sem caracteres de controle (exceto `\n`) |
| `heading(s)` | título de 1–120 caracteres, uma linha |
| `divider()` | linha separadora |
| `button(id, label)` | `id` em `[a-z0-9_-]{1,32}`; rótulo 1–80 caracteres |
| `list(itens)` | até 100 itens de até 200 caracteres |
| `action_list(itens)` / `item(texto, done, &[(id, rótulo)])` | até 100 itens; até 4 ações por item (cada ação é um botão com `id`); `done: Some(_)` mostra ☑/☐ e risca o concluído; item e ações contam no orçamento de nós |
| `stack(filhos)` / `row(filhos)` | coluna / linha; profundidade até 8; no máximo 200 nós; saída total até 64 KiB |
| `input(id, rótulo, valor)` | texto de uma linha; valor até 1000 caracteres; rótulo até 80 |
| `textarea(id, rótulo, valor)` | texto de várias linhas (`\n`); valor até 4000 caracteres; 2–12 linhas visíveis; Enter faz nova linha; a soma dos valores de um clique é limitada a 16 000 caracteres |
| `checkbox(id, rótulo, marcada)` | rótulo 1–80 caracteres |
| `select(id, rótulo, opções, escolhida)` | 1–50 opções de até 80 caracteres; `escolhida` precisa estar nas opções (vazio = a primeira) |
| `progress(0..=100, rótulo)` | barra de progresso com rótulo opcional |

Campos (`input`, `textarea`, `checkbox`, `select`) precisam de `id` **único na tela**, no máximo 32 por
tela. Saída fora do catálogo ou dos limites vira o erro "UI inválida" para o usuário e **não** é
desenhada. Não há como injetar HTML, estilo ou script. (O formato antigo do clique, sem
`values`, continua valendo.)

## Seus dados: backup

Na aba Mini-apps, cada app tem **Exportar dados** (salva um JSON legível onde você escolher) e
**Restaurar dados** (escolhe um backup e **substitui** os dados atuais do app). O backup é
validado antes de tocar em qualquer coisa: precisa ser do mesmo app, respeitar os limites de
chave/valor e a quota de 4 MiB, sem chaves repetidas; se algo estiver errado, nada muda.
Remover o app pergunta se apaga ou mantém os dados.

## Permissões

Cada interface do host é uma permissão. O manifesto deve declarar **exatamente** as que o
componente importa. Em Rust (com o SDK) isso é só o que o código usa; em outras linguagens
veja "Outras linguagens" acima.

| Permissão | O que dá | API do SDK |
|---|---|---|
| `log` | mensagens de diagnóstico (limitadas em linhas e tamanho) | `superapp_sdk::log` |
| `storage` | chave-valor próprio: chave ≤ 100 B, valor ≤ 1 MiB, total ≤ 4 MiB | `superapp_sdk::storage` |
| `clock` | milissegundos desde a época Unix | `superapp_sdk::now_ms` |
| `http` | requisições **https** pelo host, só para os domínios de `network.hosts` (ver "Internet") | `superapp_sdk::http` |

Se o código importar algo que o manifesto não declara (ou que o usuário não aprovou), o
mini-app **não carrega**, e a recusa vai para a auditoria.

## Internet (`http`)

O app não abre conexão: ele pede ao host, que só atende o que o manifesto declarou **e** o usuário aprovou
([ADR-011](../plan/04-adrs/ADR-011-miniapp-network.md)). O consentimento lista os domínios e avisa que o app
pode enviar a eles qualquer dado que lê.

```json
{ "permissions": ["storage", "http"],
  "network": { "hosts": ["api.exemplo.com", "*.reddit.com"] },
  "limits": { "timeout_ms": 20000 } }
```

```rust
let r = superapp_sdk::http::get("https://api.exemplo.com/v1/itens?q=abc")?;
// r.status, r.content_type, r.body
let r = superapp_sdk::http::post("https://api.exemplo.com/v1/itens", "application/json", b"{}")?;
```

- `network.hosts`: de 1 a 8 domínios em minúsculas; `*.x.com` vale **um** nível (`a.x.com`, não `x.com` nem
  `a.b.x.com`). Nunca IP, nunca `localhost`. Declarar `network` sem `http` (ou `http` sem `network`) invalida o pacote.
- Só `https` na porta 443, `GET` e `POST`, sem usuário/senha na URL. Cabeçalhos: só `accept` e `content-type`
  (sem `Authorization` nem cookies; chaves de API virão com o cofre de segredos).
- O host resolve o DNS e **recusa se algum IP não for público** (loopback, redes privadas, metadata de nuvem...);
  cada redirecionamento é revalidado do zero (até 3).
- Envio até 64 KiB; resposta até 1 MiB e só texto, JSON ou XML (acima disso é erro, nunca truncado).
- Até **4 chamadas por execução** e 30 por minuto por app; 10 s por chamada. O limite de execução do app
  (padrão 5 s) também vale: peça `limits.timeout_ms` maior se a rede for lenta.
- Cada chamada vai para a auditoria (host, método, status, bytes; sem a query).
- **Dica de segurança**: um app que guarda dados pessoais (como fichas de entrevista) não deve pedir `http`.
  Use outro app para coletar e passe o resultado por arquivo de importação.

## Limites de execução

Por padrão: 50 M de combustível (instruções), 16 MiB de memória, 5 s no total. O manifesto pode
pedir outros valores, até os tetos do host (500 M, 64 MB, 30 s). Estourar um limite devolve um
erro legível ("combustível esgotado", "limite de memória excedido", "tempo limite excedido") e o
host continua funcionando.

## Manifesto

```json
{
  "schema": 1,
  "id": "contador",
  "name": "Contador",
  "version": "0.1.0",
  "permissions": ["storage"],
  "limits": { "memory_mb": 16 }
}
```

- `id`: `[a-z0-9-]{1,32}` (identidade e nome do diretório de dados).
- `version`: `N.N.N`. Atualizações precisam ter versão **maior** que a instalada.
- Campos ou permissões desconhecidos **invalidam** o pacote.
- O `wasm_sha256` é calculado pelo `pack`; não escreva à mão.

## Empacotar e assinar

```bash
cargo xtask keygen minha.key                      # cria a chave privada; imprime a pública
cargo xtask pack app.wasm manifest.json minha.key app.sapp
cargo xtask verify app.sapp trusted-keys.txt      # confere assinatura, hash e manifesto
```

- **Nunca versione `*.key`** (o `.gitignore` já as ignora). Quem tem a chave publica em seu nome.
- O host só instala pacotes de autores cuja **chave pública** esteja na lista de **autores
  confiáveis** (Configurações; fica em `<dados do app>/trusted-keys.txt`, uma chave hex por
  linha, `#` comenta). Adicionar ou remover vale depois de reiniciar; remover uma chave faz os
  apps dela pararem de abrir.
- O `pack` **recusa** código com segredos conhecidos embutidos (chave privada PEM, tokens AWS,
  GitHub, Slack, Google, `sk-...`). Tire o segredo do código.
- O arquivo `.sapp` é arrastado para a janela do Superapp (ou escolhido em **Instalar de um
  arquivo**), que mostra o **consentimento** antes de instalar. Detalhes do formato: [ADR-010](../plan/04-adrs/ADR-010-package-format-signing.md).

## O que o host garante (e o que não)

Garante: fuel obrigatório; memória e tempo limitados; sem FS/rede; permissões por interface;
dados isolados por id; pacote reverificado a cada carga; UI validada; tudo auditado como
`app:<id>`. Detalhes e testes: [modelo de segurança](../plan/06-security-model.md).

Não garante (ainda): revogação por lista central (a revogação é por chave, na sua máquina), nem
isolamento de canais laterais de tempo (o relógio do WASI está disponível ao guest).
