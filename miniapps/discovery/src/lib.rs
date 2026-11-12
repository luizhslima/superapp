//! Discovery: bancada manual para a fase de descoberta de uma startup (território, hipóteses,
//! entrevistas e roteiro). Modelo de dados em `docs/dev/discovery-data-model.md` (schema 1).
//!
//! Só usa `storage` e `clock`. O estado inteiro vive no storage (cada evento roda numa instância
//! nova): registros em chaves próprias (`t1`, `h2`, `i3`), um índice por coleção (`idx:t`...),
//! `meta` (versão do esquema e contador de ids) e `ui` (tela atual).
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeMap;
use superapp_sdk::{export_app, now_ms, serde_json, storage, ui, App, Event, Node};

const SCHEMA: u32 = 1;
/// Itens por página (cada item de lista custa 1 + nº de ações no orçamento de 200 nós da UI).
const PAGINA: usize = 15;
/// Hipóteses cabem como caixas de marcação na ficha de entrevista (teto de 32 campos por tela).
const MAX_H: usize = 20;
const MAX_T: usize = 5;
const MAX_I: usize = 200;
const MSG_RO: &str = "Dados de uma versão mais nova do app: somente leitura.";

// ---------------------------------------------------------------- modelo

#[derive(Serialize, Deserialize, Clone)]
struct Meta {
    schema: u32,
    seq: u64,
}

impl Default for Meta {
    fn default() -> Self {
        Meta {
            schema: SCHEMA,
            seq: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Ui {
    tela: String,
    sel: String,
    pag: usize,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Territorio {
    nome: String,
    processo: String,
    executor: String,
    responsavel: String,
    contexto: String,
    alternativa: String,
    gatilho: String,
    em: u64,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Hipotese {
    t: String,
    texto: String,
    procura: String,
    contra: String,
    experimento: String,
    status: String,
    em: u64,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Entrevista {
    h: Vec<String>,
    participante: String,
    empresa: String,
    papel: String,
    canal: String,
    data: String,
    fato: String,
    interpretacao: String,
    duvida: String,
    observacao: bool,
    em: u64,
}

#[derive(Clone, Copy, PartialEq)]
enum Col {
    T,
    H,
    I,
}

impl Col {
    fn p(self) -> char {
        match self {
            Col::T => 't',
            Col::H => 'h',
            Col::I => 'i',
        }
    }
    fn max(self) -> usize {
        match self {
            Col::T => MAX_T,
            Col::H => MAX_H,
            Col::I => MAX_I,
        }
    }
    fn nome(self) -> &'static str {
        match self {
            Col::T => "territórios",
            Col::H => "hipóteses",
            Col::I => "entrevistas",
        }
    }
    fn de_id(id: &str) -> Option<Col> {
        match id.chars().next()? {
            't' => Some(Col::T),
            'h' => Some(Col::H),
            'i' => Some(Col::I),
            _ => None,
        }
    }
}

/// (código guardado, rótulo na tela)
type Tabela = &'static [(&'static str, &'static str)];
const PAPEIS: Tabela = &[
    ("executor", "Executor"),
    ("responsavel", "Responsável"),
    ("comprador", "Comprador"),
    ("outro", "Outro"),
];
const CANAIS: Tabela = &[
    ("indicacao", "Indicação"),
    ("comunidade", "Comunidade"),
    ("email", "E-mail"),
    ("evento", "Evento"),
    ("outro", "Outro"),
];
const STATUS: Tabela = &[
    ("aberta", "Aberta"),
    ("confirmada", "Confirmada"),
    ("refutada", "Refutada"),
    ("reformulada", "Reformulada"),
];

fn rotulo(t: Tabela, codigo: &str) -> &'static str {
    t.iter().find(|(c, _)| *c == codigo).unwrap_or(&t[0]).1
}
fn codigo(t: Tabela, rot: &str) -> &'static str {
    t.iter().find(|(_, r)| *r == rot).unwrap_or(&t[0]).0
}
fn rotulos(t: Tabela) -> Vec<&'static str> {
    t.iter().map(|(_, r)| *r).collect()
}

// ---------------------------------------------------------------- texto

/// Uma linha só, sem controles, até `max` caracteres.
fn linha(s: &str, max: usize) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .chars()
        .take(max)
        .collect()
}

/// Várias linhas (`\n`), sem outros controles, até `max` caracteres.
fn longo(s: &str, max: usize) -> String {
    s.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect::<String>()
        .trim()
        .chars()
        .take(max)
        .collect()
}

/// Corta para caber numa linha de lista, com reticências.
fn corta(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= max {
        s
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn data_valida(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let n = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<u32>().ok());
    matches!((n(0..4), n(5..7), n(8..10)), (Some(a), Some(m), Some(d))
        if a >= 2000 && (1..=12).contains(&m) && (1..=31).contains(&d))
}

// ---------------------------------------------------------------- storage

fn ler<T: DeserializeOwned>(key: &str) -> Option<T> {
    storage::get(key).and_then(|b| serde_json::from_slice(&b).ok())
}

fn gravar<T: Serialize>(key: &str, v: &T) -> Result<(), String> {
    let b = serde_json::to_vec(v).map_err(|e| e.to_string())?;
    storage::set(key, &b)
}

fn chave_indice(c: Col) -> String {
    format!("idx:{}", c.p())
}

fn indice(c: Col) -> Vec<String> {
    ler(&chave_indice(c)).unwrap_or_default()
}

/// Cria um registro: contador, registro e, por último, o índice (uma queda no meio deixa no pior
/// caso um órfão, que o reparo da abertura reinclui).
fn criar<T: Serialize>(c: Col, v: &T) -> Result<String, String> {
    let mut idx = indice(c);
    if idx.len() >= c.max() {
        return Err(format!("Limite de {} {} atingido.", c.max(), c.nome()));
    }
    let mut meta: Meta = ler("meta").unwrap_or_default();
    meta.seq += 1;
    gravar("meta", &meta)?;
    let id = format!("{}{}", c.p(), meta.seq);
    gravar(&id, v)?;
    idx.push(id.clone());
    gravar(&chave_indice(c), &idx)?;
    Ok(id)
}

/// Remove: primeiro do índice, depois o registro.
fn apagar(c: Col, id: &str) -> Result<(), String> {
    let mut idx = indice(c);
    idx.retain(|x| x != id);
    gravar(&chave_indice(c), &idx)?;
    storage::delete(id)
}

/// Reparo da abertura: tira do índice ids sem registro e reinclui órfãos (1..=seq).
fn reparar() -> Result<(), String> {
    let meta: Meta = ler("meta").unwrap_or_default();
    for c in [Col::T, Col::H, Col::I] {
        let antes = indice(c);
        let mut idx: Vec<String> = antes
            .iter()
            .filter(|id| storage::get(id).is_some())
            .cloned()
            .collect();
        for n in 1..=meta.seq {
            let id = format!("{}{}", c.p(), n);
            if !idx.contains(&id) && storage::get(&id).is_some() && idx.len() < c.max() {
                idx.push(id);
            }
        }
        if idx != antes {
            gravar(&chave_indice(c), &idx)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- rascunho e contexto

enum Rascunho {
    Nenhum,
    T(Territorio),
    H(Hipotese),
    I(Entrevista),
}

struct Ctx {
    aviso: Option<String>,
    rascunho: Rascunho,
    somente_leitura: bool,
}

type Valores = BTreeMap<String, String>;

fn v<'a>(values: &'a Valores, k: &str) -> &'a str {
    values.get(k).map(String::as_str).unwrap_or("")
}

fn territorio_de(values: &Valores, em: u64) -> Territorio {
    Territorio {
        nome: linha(v(values, "f-nome"), 80),
        processo: linha(v(values, "f-processo"), 300),
        executor: linha(v(values, "f-executor"), 300),
        responsavel: linha(v(values, "f-responsavel"), 300),
        contexto: linha(v(values, "f-contexto"), 300),
        alternativa: linha(v(values, "f-alternativa"), 300),
        gatilho: linha(v(values, "f-gatilho"), 300),
        em,
    }
}

fn hipotese_de(values: &Valores, em: u64) -> Hipotese {
    let t = v(values, "f-t").split(' ').next().unwrap_or("").to_string();
    Hipotese {
        t,
        texto: longo(v(values, "f-texto"), 500),
        procura: longo(v(values, "f-procura"), 500),
        contra: longo(v(values, "f-contra"), 500),
        experimento: longo(v(values, "f-experimento"), 500),
        status: codigo(STATUS, v(values, "f-status")).to_string(),
        em,
    }
}

fn entrevista_de(values: &Valores, em: u64) -> Entrevista {
    let validas = indice(Col::H);
    let h = values
        .iter()
        .filter(|(_, val)| val.as_str() == "true")
        .filter_map(|(k, _)| k.strip_prefix("hk-"))
        .filter(|id| validas.iter().any(|x| x == id))
        .map(str::to_string)
        .collect();
    Entrevista {
        h,
        participante: linha(v(values, "f-part"), 20),
        empresa: linha(v(values, "f-emp"), 80),
        papel: codigo(PAPEIS, v(values, "f-papel")).to_string(),
        canal: codigo(CANAIS, v(values, "f-canal")).to_string(),
        data: linha(v(values, "f-data"), 10),
        fato: longo(v(values, "f-fato"), 4000),
        interpretacao: longo(v(values, "f-interp"), 4000),
        duvida: longo(v(values, "f-duvida"), 4000),
        observacao: v(values, "f-obs") == "true",
        em,
    }
}

// ---------------------------------------------------------------- eventos

fn lista_da(tela: &str) -> &'static str {
    match tela {
        "form-t" => "territorios",
        "form-h" => "hipoteses",
        _ => "entrevistas",
    }
}

fn clique(ui: &mut Ui, ctx: &mut Ctx, id: &str, values: &Valores) {
    match id {
        "nav-t" | "nav-h" | "nav-i" | "nav-r" => {
            ui.tela = match id {
                "nav-t" => "territorios",
                "nav-h" => "hipoteses",
                "nav-i" => "entrevistas",
                _ => "roteiro",
            }
            .into();
            ui.sel.clear();
            ui.pag = 0;
        }
        "pag-ant" => ui.pag = ui.pag.saturating_sub(1),
        "pag-prox" => ui.pag += 1,
        "novo" => {
            if ctx.somente_leitura {
                ctx.aviso = Some(MSG_RO.into());
            } else {
                ui.tela = match ui.tela.as_str() {
                    "territorios" => "form-t",
                    "hipoteses" => "form-h",
                    _ => "form-i",
                }
                .into();
                ui.sel.clear();
            }
        }
        "cancelar" | "nao" => {
            ui.tela = lista_da(&ui.tela_anterior()).into();
            ui.sel.clear();
        }
        "salvar" => salvar(ui, ctx, values),
        "sim" => remover(ui, ctx),
        outro => {
            if let Some(alvo) = outro.strip_prefix("ab-") {
                if let Some(c) = Col::de_id(alvo) {
                    ui.tela = match c {
                        Col::T => "form-t",
                        Col::H => "form-h",
                        Col::I => "form-i",
                    }
                    .into();
                    ui.sel = alvo.into();
                }
            } else if let Some(alvo) = outro.strip_prefix("rm-") {
                if ctx.somente_leitura {
                    ctx.aviso = Some(MSG_RO.into());
                } else if Col::de_id(alvo).is_some() && storage::get(alvo).is_some() {
                    ui.tela = "confirma".into();
                    ui.sel = alvo.into();
                }
            }
        }
    }
}

impl Ui {
    /// Tela de origem de uma ficha/confirmação, para o "cancelar" voltar à lista certa.
    fn tela_anterior(&self) -> String {
        match self.tela.as_str() {
            "confirma" => match Col::de_id(&self.sel) {
                Some(Col::T) => "form-t",
                Some(Col::H) => "form-h",
                _ => "form-i",
            }
            .into(),
            t => t.into(),
        }
    }
}

fn salvar(ui: &mut Ui, ctx: &mut Ctx, values: &Valores) {
    if ctx.somente_leitura {
        ctx.aviso = Some(MSG_RO.into());
        return;
    }
    let novo = ui.sel.is_empty();
    let agora = now_ms();
    let resultado: Result<(), String> = match ui.tela.as_str() {
        "form-t" => {
            let em = if novo {
                agora
            } else {
                ler::<Territorio>(&ui.sel).map(|t| t.em).unwrap_or(agora)
            };
            let t = territorio_de(values, em);
            if t.nome.is_empty() {
                ctx.rascunho = Rascunho::T(t);
                Err("Dê um nome ao território.".into())
            } else {
                let r = if novo {
                    criar(Col::T, &t).map(|_| ())
                } else {
                    gravar(&ui.sel, &t)
                };
                if r.is_err() {
                    ctx.rascunho = Rascunho::T(t);
                }
                r
            }
        }
        "form-h" => {
            let em = if novo {
                agora
            } else {
                ler::<Hipotese>(&ui.sel).map(|h| h.em).unwrap_or(agora)
            };
            let h = hipotese_de(values, em);
            if h.texto.is_empty() {
                ctx.rascunho = Rascunho::H(h);
                Err("Escreva a hipótese.".into())
            } else if ler::<Territorio>(&h.t).is_none() {
                ctx.rascunho = Rascunho::H(h);
                Err("Escolha um território existente.".into())
            } else {
                let r = if novo {
                    criar(Col::H, &h).map(|_| ())
                } else {
                    gravar(&ui.sel, &h)
                };
                if r.is_err() {
                    ctx.rascunho = Rascunho::H(h);
                }
                r
            }
        }
        _ => {
            let em = if novo {
                agora
            } else {
                ler::<Entrevista>(&ui.sel).map(|e| e.em).unwrap_or(agora)
            };
            let e = entrevista_de(values, em);
            if e.participante.is_empty() {
                ctx.rascunho = Rascunho::I(e);
                Err("Informe o código do participante (ex.: P3).".into())
            } else if !e.data.is_empty() && !data_valida(&e.data) {
                ctx.rascunho = Rascunho::I(e);
                Err("Data no formato AAAA-MM-DD (ou deixe em branco).".into())
            } else {
                let r = if novo {
                    criar(Col::I, &e).map(|_| ())
                } else {
                    gravar(&ui.sel, &e)
                };
                if r.is_err() {
                    ctx.rascunho = Rascunho::I(e);
                }
                r
            }
        }
    };
    match resultado {
        Ok(()) => {
            ui.tela = lista_da(&ui.tela).into();
            ui.sel.clear();
        }
        Err(e) => ctx.aviso = Some(e),
    }
}

fn remover(ui: &mut Ui, ctx: &mut Ctx) {
    if ctx.somente_leitura {
        ctx.aviso = Some(MSG_RO.into());
        return;
    }
    let id = ui.sel.clone();
    let Some(c) = Col::de_id(&id) else { return };
    let r: Result<(), String> = match c {
        Col::T => {
            let em_uso = indice(Col::H)
                .iter()
                .any(|h| ler::<Hipotese>(h).is_some_and(|h| h.t == id));
            if em_uso {
                Err("Há hipóteses neste território: remova ou mova antes.".into())
            } else {
                apagar(Col::T, &id)
            }
        }
        Col::H => {
            // Desliga a hipótese das entrevistas (nunca apaga entrevista em cascata).
            let mut r = Ok(());
            for i in indice(Col::I) {
                if let Some(mut e) = ler::<Entrevista>(&i) {
                    if e.h.contains(&id) {
                        e.h.retain(|x| x != &id);
                        r = r.and(gravar(&i, &e));
                    }
                }
            }
            r.and_then(|_| apagar(Col::H, &id))
        }
        Col::I => apagar(Col::I, &id),
    };
    if let Err(e) = r {
        ctx.aviso = Some(e);
    }
    ui.tela = match c {
        Col::T => "territorios",
        Col::H => "hipoteses",
        Col::I => "entrevistas",
    }
    .into();
    ui.sel.clear();
}

// ---------------------------------------------------------------- telas

fn nav() -> Node {
    ui::row(vec![
        ui::button("nav-t", "Territórios"),
        ui::button("nav-h", "Hipóteses"),
        ui::button("nav-i", "Entrevistas"),
        ui::button("nav-r", "Roteiro"),
    ])
}

fn pagina(total: usize, pag: usize) -> (usize, usize, usize) {
    let ultima = total.saturating_sub(1) / PAGINA;
    let p = pag.min(ultima);
    (p, p * PAGINA, ultima)
}

fn lista(
    c: Col,
    ui_: &Ui,
    itens: impl Fn(&str) -> Option<String>,
    vazio: &str,
    novo: &str,
) -> Vec<Node> {
    let idx = indice(c);
    let (p, ini, ultima) = pagina(idx.len(), ui_.pag);
    let mut f = vec![ui::row(vec![ui::button("novo", novo)])];
    if idx.is_empty() {
        f.push(ui::text(vazio));
        return f;
    }
    let linhas = idx
        .iter()
        .skip(ini)
        .take(PAGINA)
        .filter_map(|id| {
            let texto = itens(id)?;
            let (ab, rm) = (format!("ab-{id}"), format!("rm-{id}"));
            Some(ui::item(texto, None, &[(&ab, "Abrir"), (&rm, "Remover")]))
        })
        .collect();
    f.push(ui::action_list(linhas));
    if ultima > 0 {
        let mut nav = vec![];
        if p > 0 {
            nav.push(ui::button("pag-ant", "Anterior"));
        }
        nav.push(ui::text(format!("Página {} de {}", p + 1, ultima + 1)));
        if p < ultima {
            nav.push(ui::button("pag-prox", "Próxima"));
        }
        f.push(ui::row(nav));
    }
    f
}

fn tela_territorios(u: &Ui) -> Vec<Node> {
    let mut f = vec![ui::text(
        "Território: o processo e o público provisórios que você vai investigar. Comece pequeno.",
    )];
    f.extend(lista(
        Col::T,
        u,
        |id| {
            let t: Territorio = ler(id)?;
            Some(format!("{id} · {} · {}", t.nome, corta(&t.processo, 80)))
        },
        "Nenhum território ainda.",
        "Novo território",
    ));
    f
}

fn tela_hipoteses(u: &Ui) -> Vec<Node> {
    let mut f = vec![ui::text(
        "Hipótese: algo que você acredita sobre o problema e pode ser refutado por evidência.",
    )];
    f.extend(lista(
        Col::H,
        u,
        |id| {
            let h: Hipotese = ler(id)?;
            Some(format!(
                "{id} [{}] {}",
                rotulo(STATUS, &h.status),
                corta(&h.texto, 120)
            ))
        },
        "Nenhuma hipótese ainda.",
        "Nova hipótese",
    ));
    f
}

fn tela_entrevistas(u: &Ui) -> Vec<Node> {
    let mut f = vec![ui::text(
        "Ficha por entrevista: separe o que aconteceu (fato) do que você acha (interpretação).",
    )];
    f.extend(lista(
        Col::I,
        u,
        |id| {
            let e: Entrevista = ler(id)?;
            let data = if e.data.is_empty() {
                "sem data"
            } else {
                &e.data
            };
            Some(format!(
                "{id} · {} · {} · {} · {data}",
                e.participante,
                if e.empresa.is_empty() {
                    "sem empresa"
                } else {
                    &e.empresa
                },
                rotulo(PAPEIS, &e.papel).to_lowercase()
            ))
        },
        "Nenhuma entrevista ainda.",
        "Nova entrevista",
    ));
    f
}

const ROTEIRO: &[(&str, &[&str])] = &[
    (
        "Abertura",
        &["Estou investigando como esse processo funciona hoje; ainda não sei se há um problema relevante. Quero entender sua experiência, inclusive o que já funciona bem."],
    ),
    (
        "Contexto e episódio",
        &[
            "Qual é a sua responsabilidade nesse processo?",
            "Conte sobre a última vez que você precisou executá-lo.",
            "O que disparou o trabalho?",
            "Pode reconstruir as etapas, do início ao fim?",
            "Quem mais participou?",
        ],
    ),
    (
        "Dificuldades e consequências",
        &[
            "Em qual etapa houve mais esforço, espera ou retrabalho?",
            "O que aconteceu por causa disso?",
            "Como vocês perceberam o problema?",
            "Isso aconteceu em outras ocasiões? Quais?",
            "Existe alguma medida desse impacto, ou é uma estimativa?",
        ],
    ),
    (
        "Alternativas e prioridade",
        &[
            "O que vocês já tentaram mudar?",
            "O que funcionou e o que não funcionou?",
            "Por que continuam usando a alternativa atual?",
            "Que outros problemas competem por atenção e orçamento?",
            "Quem precisaria concordar com uma mudança?",
        ],
    ),
    (
        "Fechamento",
        &[
            "Quem teria uma visão diferente da sua?",
            "Seria possível observar uma execução ou ver um exemplo anonimizado?",
            "Posso voltar para esclarecer algum ponto?",
        ],
    ),
];

fn tela_roteiro() -> Vec<Node> {
    let mut f = vec![
        ui::text("Pergunte sobre experiências passadas, não sobre a sua ideia."),
        ui::text("Evite: \"Você usaria uma ferramenta que resolvesse isso?\" Prefira: \"O que aconteceu quando vocês tentaram resolver isso da última vez?\""),
    ];
    for (titulo, perguntas) in ROTEIRO {
        f.push(ui::heading(*titulo));
        f.push(ui::list(perguntas.iter().copied()));
    }
    f
}

fn botoes_ficha() -> Node {
    ui::row(vec![
        ui::button("salvar", "Salvar"),
        ui::button("cancelar", "Cancelar"),
    ])
}

fn ficha_territorio(sel: &str, r: &Rascunho) -> Vec<Node> {
    let t = match r {
        Rascunho::T(t) => t.clone(),
        _ => ler(sel).unwrap_or_default(),
    };
    vec![
        ui::heading(if sel.is_empty() {
            "Novo território"
        } else {
            "Território"
        }),
        ui::input("f-nome", "Nome (até 80)", t.nome),
        ui::input("f-processo", "Processo investigado", t.processo),
        ui::input("f-executor", "Quem executa o trabalho", t.executor),
        ui::input(
            "f-responsavel",
            "Quem responde pelo resultado",
            t.responsavel,
        ),
        ui::input("f-contexto", "Contexto", t.contexto),
        ui::input("f-alternativa", "Alternativa atual", t.alternativa),
        ui::input("f-gatilho", "O que torna urgente (gatilho)", t.gatilho),
        botoes_ficha(),
    ]
}

fn ficha_hipotese(sel: &str, r: &Rascunho) -> Vec<Node> {
    let h = match r {
        Rascunho::H(h) => h.clone(),
        _ => ler(sel).unwrap_or_default(),
    };
    let territorios: Vec<(String, String)> = indice(Col::T)
        .into_iter()
        .filter_map(|id| {
            let t: Territorio = ler(&id)?;
            Some((id.clone(), format!("{id} · {}", corta(&t.nome, 60))))
        })
        .collect();
    if territorios.is_empty() {
        return vec![
            ui::text("Crie um território antes de escrever hipóteses."),
            ui::button("cancelar", "Voltar"),
        ];
    }
    let escolhido = territorios
        .iter()
        .find(|(id, _)| *id == h.t)
        .map(|(_, r)| r.clone())
        .unwrap_or_else(|| territorios[0].1.clone());
    vec![
        ui::heading(if sel.is_empty() {
            "Nova hipótese"
        } else {
            "Hipótese"
        }),
        ui::select(
            "f-t",
            "Território",
            territorios.iter().map(|(_, r)| r.clone()),
            escolhido,
        ),
        ui::textarea("f-texto", "Hipótese (até 500)", h.texto),
        ui::textarea("f-procura", "Evidência que procuro", h.procura),
        ui::textarea("f-contra", "Evidência que a contradiria", h.contra),
        ui::textarea("f-experimento", "Experimento", h.experimento),
        ui::select(
            "f-status",
            "Status",
            rotulos(STATUS),
            rotulo(STATUS, &h.status),
        ),
        botoes_ficha(),
    ]
}

fn ficha_entrevista(sel: &str, r: &Rascunho) -> Vec<Node> {
    let e = match r {
        Rascunho::I(e) => e.clone(),
        _ => ler(sel).unwrap_or_default(),
    };
    let mut f = vec![
        ui::heading(if sel.is_empty() {
            "Nova entrevista"
        } else {
            "Entrevista"
        }),
        ui::row(vec![
            ui::input("f-part", "Participante (código, ex.: P3)", e.participante),
            ui::input("f-emp", "Empresa (opcional)", e.empresa),
        ]),
        ui::row(vec![
            ui::input("f-data", "Data (AAAA-MM-DD)", e.data),
            ui::select(
                "f-papel",
                "Papel",
                rotulos(PAPEIS),
                rotulo(PAPEIS, &e.papel),
            ),
            ui::select(
                "f-canal",
                "Canal",
                rotulos(CANAIS),
                rotulo(CANAIS, &e.canal),
            ),
        ]),
        ui::checkbox(
            "f-obs",
            "Houve observação autorizada do trabalho",
            e.observacao,
        ),
        ui::textarea("f-fato", "Fato: o que aconteceu, nas palavras dela", e.fato),
        ui::textarea("f-interp", "Interpretação: o que eu acho", e.interpretacao),
        ui::textarea("f-duvida", "Dúvidas que ficaram", e.duvida),
    ];
    let hs: Vec<(String, Hipotese)> = indice(Col::H)
        .into_iter()
        .filter_map(|id| Some((id.clone(), ler::<Hipotese>(&id)?)))
        .collect();
    if hs.is_empty() {
        f.push(ui::text(
            "Sem hipóteses ainda: crie algumas para ligá-las às entrevistas.",
        ));
    } else {
        f.push(ui::text("Hipóteses testadas nesta conversa:"));
        for (id, h) in hs {
            f.push(ui::checkbox(
                format!("hk-{id}"),
                format!("{id} · {}", corta(&h.texto, 70)),
                e.h.contains(&id),
            ));
        }
    }
    f.push(botoes_ficha());
    f
}

fn tela_confirma(sel: &str) -> Vec<Node> {
    let que = match Col::de_id(sel) {
        Some(Col::T) => "o território",
        Some(Col::H) => "a hipótese (as entrevistas continuam; só perdem a ligação)",
        _ => "a entrevista e a sua ficha",
    };
    vec![
        ui::heading("Remover?"),
        ui::text(format!(
            "Remover {que} {sel}? Não dá para desfazer (só restaurando um backup)."
        )),
        ui::row(vec![
            ui::button("sim", "Remover"),
            ui::button("nao", "Cancelar"),
        ]),
    ]
}

fn tela(u: &Ui, ctx: &Ctx) -> Node {
    let mut f = vec![ui::heading("Discovery"), nav()];
    if let Some(a) = &ctx.aviso {
        f.push(ui::text(a.clone()));
    }
    f.push(ui::divider());
    f.extend(match u.tela.as_str() {
        "hipoteses" => tela_hipoteses(u),
        "entrevistas" => tela_entrevistas(u),
        "roteiro" => tela_roteiro(),
        "form-t" => ficha_territorio(&u.sel, &ctx.rascunho),
        "form-h" => ficha_hipotese(&u.sel, &ctx.rascunho),
        "form-i" => ficha_entrevista(&u.sel, &ctx.rascunho),
        "confirma" => tela_confirma(&u.sel),
        _ => tela_territorios(u),
    });
    ui::stack(f)
}

// ---------------------------------------------------------------- entrada

struct Discovery;

impl App for Discovery {
    fn handle(event: Event) -> Result<Node, String> {
        let meta: Meta = ler("meta").unwrap_or_default();
        let mut u: Ui = ler("ui").unwrap_or_default();
        let mut ctx = Ctx {
            aviso: None,
            rascunho: Rascunho::Nenhum,
            somente_leitura: meta.schema > SCHEMA,
        };
        match &event {
            Event::Init => {
                if ctx.somente_leitura {
                    ctx.aviso = Some(MSG_RO.into());
                } else if let Err(e) = reparar() {
                    ctx.aviso = Some(format!("Reparo do índice falhou: {e}"));
                }
            }
            Event::Click { id, values } => clique(&mut u, &mut ctx, id, values),
        }
        if u.tela.is_empty() {
            u.tela = "territorios".into();
        }
        if !ctx.somente_leitura {
            // Gravar a tela é conveniência: falha de quota não derruba a execução.
            let _ = gravar("ui", &u);
        }
        Ok(tela(&u, &ctx))
    }
}

export_app!(Discovery);
