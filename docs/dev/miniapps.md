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
        if matches!(&event, Event::Click { id } if id == "mais") {
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
| `Click { id }` | o usuário clica num botão com esse `id` |

| Componente (`ui::`) | Limites |
|---|---|
| `text(s)` | até 500 caracteres, sem caracteres de controle (exceto `\n`) |
| `button(id, label)` | `id` em `[a-z0-9_-]{1,32}`; rótulo 1–80 caracteres |
| `list(itens)` | até 100 itens de até 200 caracteres |
| `stack(filhos)` | profundidade até 8; no máximo 200 nós; saída total até 64 KiB |

Saída fora do catálogo ou dos limites vira o erro "UI inválida" para o usuário e **não** é
desenhada. Não há como injetar HTML, estilo ou script.

## Permissões

Cada interface do host é uma permissão. O manifesto deve declarar **exatamente** as que o
componente importa. Em Rust (com o SDK) isso é só o que o código usa; em outras linguagens
veja "Outras linguagens" acima.

| Permissão | O que dá | API do SDK |
|---|---|---|
| `log` | mensagens de diagnóstico (limitadas em linhas e tamanho) | `superapp_sdk::log` |
| `storage` | chave-valor próprio: chave ≤ 100 B, valor ≤ 1 MiB, total ≤ 4 MiB | `superapp_sdk::storage` |
| `clock` | milissegundos desde a época Unix | `superapp_sdk::now_ms` |

Se o código importar algo que o manifesto não declara (ou que o usuário não aprovou), o
mini-app **não carrega**, e a recusa vai para a auditoria.

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
- O host só instala pacotes de autores cuja **chave pública** esteja em
  `<dados do app>/trusted-keys.txt` (uma chave hex por linha, `#` comenta).
- O `pack` **recusa** código com segredos conhecidos embutidos (chave privada PEM, tokens AWS,
  GitHub, Slack, Google, `sk-...`). Tire o segredo do código.
- O arquivo `.sapp` é arrastado para a janela do Superapp, que mostra o **consentimento** antes
  de instalar. Detalhes do formato: [ADR-010](../plan/04-adrs/ADR-010-package-format-signing.md).

## O que o host garante (e o que não)

Garante: fuel obrigatório; memória e tempo limitados; sem FS/rede; permissões por interface;
dados isolados por id; pacote reverificado a cada carga; UI validada; tudo auditado como
`app:<id>`. Detalhes e testes: [modelo de segurança](../plan/06-security-model.md).

Não garante (ainda): revogação por lista central, atualização automática, nem isolamento de
canais laterais de tempo (o relógio do WASI está disponível ao guest).
