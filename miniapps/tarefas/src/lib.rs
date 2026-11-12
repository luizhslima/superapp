//! Tarefas: lista de afazeres. Exemplo de mini-app com campo de texto, linha de componentes,
//! lista com ações por item e barra de progresso. Só usa `storage`.
//!
//! Também é o gabarito que `cargo xtask new-app` copia: troque o conteúdo de `handle`/`tela`
//! pela lógica do seu app e mantenha o padrão (carregar → reagir ao evento → salvar → desenhar).
use superapp_sdk::{export_app, storage, ui, App, Event, Node};

/// Tudo fica numa chave só: uma tarefa por linha, `0|texto` (aberta) ou `1|texto` (feita).
const KEY: &str = "tarefas";
/// Cabe no orçamento de 200 nós da UI (cada item conta 3: ele e as duas ações).
const MAX_TAREFAS: usize = 50;
const MAX_CHARS: usize = 200;

struct Tarefa {
    feita: bool,
    texto: String,
}

fn carregar() -> Vec<Tarefa> {
    storage::get(KEY)
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (f, t) = l.split_once('|')?;
            Some(Tarefa {
                feita: f == "1",
                texto: t.to_string(),
            })
        })
        .collect()
}

fn salvar(ts: &[Tarefa]) -> Result<(), String> {
    let s = ts
        .iter()
        .map(|t| format!("{}|{}", u8::from(t.feita), t.texto))
        .collect::<Vec<_>>()
        .join("\n");
    storage::set(KEY, s.as_bytes())
}

/// Uma linha só, sem controles, no tamanho que a UI aceita.
fn limpar(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_CHARS)
        .collect()
}

fn indice(id: &str, prefixo: &str) -> Option<usize> {
    id.strip_prefix(prefixo)?.parse().ok()
}

fn tela(ts: &[Tarefa], aviso: Option<&str>) -> Node {
    let mut filhos = vec![
        ui::heading("Tarefas"),
        ui::row(vec![
            ui::input("nova", "Nova tarefa", ""),
            ui::button("add", "Adicionar"),
        ]),
    ];
    if let Some(a) = aviso {
        filhos.push(ui::text(a));
    }
    if ts.is_empty() {
        filhos.push(ui::text("Nenhuma tarefa ainda."));
        return ui::stack(filhos);
    }
    let feitas = ts.iter().filter(|t| t.feita).count();
    filhos.push(ui::progress(
        (feitas * 100 / ts.len()) as u8,
        format!("{feitas} de {} concluídas", ts.len()),
    ));
    let itens = ts
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let (feito, remover) = (format!("feito-{i}"), format!("rm-{i}"));
            ui::item(
                t.texto.clone(),
                Some(t.feita),
                &[
                    (&feito, if t.feita { "Desfazer" } else { "Feito" }),
                    (&remover, "Remover"),
                ],
            )
        })
        .collect();
    filhos.push(ui::action_list(itens));
    if feitas > 0 {
        filhos.push(ui::button("limpar", "Limpar concluídas"));
    }
    ui::stack(filhos)
}

struct Tarefas;

impl App for Tarefas {
    fn handle(event: Event) -> Result<Node, String> {
        let mut ts = carregar();
        let mut aviso = None;
        if let Event::Click { id, values } = &event {
            let mut mudou = true;
            match id.as_str() {
                "add" => {
                    let texto = values.get("nova").map(|s| limpar(s)).unwrap_or_default();
                    if texto.is_empty() {
                        aviso = Some("Digite a tarefa antes de adicionar.");
                        mudou = false;
                    } else if ts.len() >= MAX_TAREFAS {
                        aviso = Some("Limite de 50 tarefas: conclua e limpe algumas.");
                        mudou = false;
                    } else {
                        ts.push(Tarefa {
                            feita: false,
                            texto,
                        });
                    }
                }
                "limpar" => ts.retain(|t| !t.feita),
                outro => match (indice(outro, "feito-"), indice(outro, "rm-")) {
                    (Some(i), _) if i < ts.len() => ts[i].feita = !ts[i].feita,
                    (_, Some(i)) if i < ts.len() => {
                        ts.remove(i);
                    }
                    _ => mudou = false,
                },
            }
            if mudou {
                salvar(&ts)?;
            }
        }
        Ok(tela(&ts, aviso))
    }
}

export_app!(Tarefas);
