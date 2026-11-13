//! Discovery: bancada manual para a fase de descoberta de uma startup (território, hipóteses,
//! entrevistas e roteiro). Modelo de dados em `docs/dev/discovery-data-model.md` (schema 1).
//!
//! Só usa `storage` e `clock`. O estado inteiro vive no storage (cada evento roda numa instância
//! nova): registros em chaves próprias (`t1`, `h2`, `i3`), um índice por coleção (`idx:t`...),
//! `meta` (versão do esquema e contador de ids) e `ui` (tela atual).
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use superapp_sdk::{export_app, now_ms, serde_json, storage, ui, App, Event, Node};

const SCHEMA: u32 = 1;
/// Itens por página (cada item de lista custa 1 + nº de ações no orçamento de 200 nós da UI).
const PAGINA: usize = 15;
/// Hipóteses cabem como caixas de marcação na ficha de entrevista (teto de 32 campos por tela).
const MAX_H: usize = 20;
const MAX_T: usize = 5;
const MAX_I: usize = 200;
const MAX_V: usize = 500;
const MAX_D: usize = 100;
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
    V,
    D,
}

impl Col {
    fn p(self) -> char {
        match self {
            Col::T => 't',
            Col::H => 'h',
            Col::I => 'i',
            Col::V => 'v',
            Col::D => 'd',
        }
    }
    fn max(self) -> usize {
        match self {
            Col::T => MAX_T,
            Col::H => MAX_H,
            Col::I => MAX_I,
            Col::V => MAX_V,
            Col::D => MAX_D,
        }
    }
    fn nome(self) -> &'static str {
        match self {
            Col::T => "territórios",
            Col::H => "hipóteses",
            Col::I => "entrevistas",
            Col::V => "evidências",
            Col::D => "decisões",
        }
    }
    fn de_id(id: &str) -> Option<Col> {
        match id.chars().next()? {
            't' => Some(Col::T),
            'h' => Some(Col::H),
            'i' => Some(Col::I),
            'v' => Some(Col::V),
            'd' => Some(Col::D),
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
    for c in [Col::T, Col::H, Col::I, Col::V, Col::D] {
        let antes = indice(c);
        let mut idx: Vec<String> = antes
            .iter()
            .filter(|id| storage::get(id).is_some())
            .cloned()
            .collect();
        let conhecidos: HashSet<String> = idx.iter().cloned().collect();
        for n in 1..=meta.seq {
            let id = format!("{}{}", c.p(), n);
            if !conhecidos.contains(&id) && storage::get(&id).is_some() && idx.len() < c.max() {
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
    V(Evidencia),
    D(Decisao),
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
        "form-v" => "evidencias",
        "form-d" => "decisoes",
        _ => "entrevistas",
    }
}

fn clique(ui: &mut Ui, ctx: &mut Ctx, id: &str, values: &Valores) {
    match id {
        "nav-home" | "nav-t" | "nav-h" | "nav-i" | "nav-v" | "nav-d" | "nav-p" | "nav-r" => {
            ui.tela = match id {
                "nav-home" => "inicio",
                "nav-t" => "territorios",
                "nav-h" => "hipoteses",
                "nav-i" => "entrevistas",
                "nav-v" => "evidencias",
                "nav-d" => "decisoes",
                "nav-p" => "painel",
                _ => "roteiro",
            }
            .into();
            ui.sel.clear();
            ui.pag = 0;
        }
        // Atalhos do "próximo passo" da tela Início.
        "go-form-t" | "go-form-h" | "go-form-i" | "go-form-v" | "go-form-d" => {
            if ctx.somente_leitura {
                ctx.aviso = Some(MSG_RO.into());
            } else {
                ui.tela = id.strip_prefix("go-").unwrap_or("inicio").into();
                ui.sel.clear();
            }
        }
        "go-painel" => {
            ui.tela = "painel".into();
            ui.sel.clear();
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
                    "evidencias" => "form-v",
                    "decisoes" => "form-d",
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
                        Col::V => "form-v",
                        Col::D => "form-d",
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
                Some(Col::V) => "form-v",
                Some(Col::D) => "form-d",
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
        "form-v" => salvar_evidencia(&ui.sel, ctx, values, novo, agora),
        "form-d" => salvar_decisao(&ui.sel, ctx, values, novo, agora),
        "form-i" => {
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
        _ => Err("Nada para salvar nesta tela.".into()),
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
        Col::H if hipotese_tem_evidencia(&id) => {
            Err("Há evidências ligadas a esta hipótese: remova ou mova antes.".into())
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
        Col::I => remover_entrevista(&id),
        Col::V => apagar(Col::V, &id),
        Col::D => apagar(Col::D, &id),
    };
    if let Err(e) = r {
        ctx.aviso = Some(e);
    }
    ui.tela = match c {
        Col::T => "territorios",
        Col::H => "hipoteses",
        Col::I => "entrevistas",
        Col::V => "evidencias",
        Col::D => "decisoes",
    }
    .into();
    ui.sel.clear();
}

// ---------------------------------------------------------------- telas

fn nav() -> Node {
    ui::stack(vec![
        ui::row(vec![
            ui::button("nav-home", "Início"),
            ui::button("nav-t", "Territórios"),
            ui::button("nav-h", "Hipóteses"),
            ui::button("nav-i", "Entrevistas"),
            ui::button("nav-v", "Evidências"),
        ]),
        ui::row(vec![
            ui::button("nav-d", "Decisões"),
            ui::button("nav-p", "Painel"),
            ui::button("nav-r", "Roteiro"),
        ]),
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
        Some(Col::V) => "a evidência",
        Some(Col::D) => "a decisão",
        _ => "a entrevista, a sua ficha e as evidências que a citam como fonte",
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
    if let Some(h) = ajuda_form(&u.tela) {
        f.push(ui::text(h));
    }
    f.extend(match u.tela.as_str() {
        "hipoteses" => tela_hipoteses(u),
        "entrevistas" => tela_entrevistas(u),
        "roteiro" => tela_roteiro(),
        "territorios" => tela_territorios(u),
        "evidencias" => tela_evidencias(u),
        "decisoes" => tela_decisoes(u),
        "painel" => tela_painel(),
        "form-v" => ficha_evidencia(&u.sel, &ctx.rascunho),
        "form-d" => ficha_decisao(&u.sel, &ctx.rascunho),
        "form-t" => ficha_territorio(&u.sel, &ctx.rascunho),
        "form-h" => ficha_hipotese(&u.sel, &ctx.rascunho),
        "form-i" => ficha_entrevista(&u.sel, &ctx.rascunho),
        "confirma" => tela_confirma(&u.sel),
        _ => tela_inicio(),
    });
    let mut raiz = ui::stack(f);
    dicas(&mut raiz);
    raiz
}

// ---------------------------------------------------------------- D3: evidências, decisões, painel

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Evidencia {
    h: String,
    fonte: String,
    resumo: String,
    forca: u8,
    contra: bool,
    empresa: String,
    papel: String,
    em: u64,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Decisao {
    t: String,
    h: Vec<String>,
    escolha: String,
    porque: String,
    proximo: String,
    em: u64,
}

/// Escala de força da evidência (do doc de pesquisa); o número inicial é o valor guardado.
const FORCAS: &[&str] = &[
    "1 · Reclamação pública",
    "2 · Entrevista detalhada",
    "3 · Observação autorizada",
    "4 · Tentativa anterior de solução",
    "5 · Acesso ao responsável",
    "6 · Piloto com escopo",
    "7 · Pagamento",
];
const ESCOLHAS: Tabela = &[
    ("avancar", "Avançar"),
    ("restringir", "Restringir o segmento"),
    ("reformular", "Reformular"),
    ("abandonar", "Abandonar"),
];

fn forca_de(rot: &str) -> u8 {
    rot.chars()
        .next()
        .and_then(|c| c.to_digit(10))
        .filter(|d| (1..=7).contains(d))
        .map(|d| d as u8)
        .unwrap_or(1)
}

fn forca_rotulo(f: u8) -> &'static str {
    FORCAS[(f.clamp(1, 7) - 1) as usize]
}

fn primeiro_token(s: &str) -> String {
    s.split(' ').next().unwrap_or("").to_string()
}

fn evidencia_de(values: &Valores, em: u64) -> Evidencia {
    let fonte = linha(v(values, "f-fonte"), 300);
    let mut e = Evidencia {
        h: primeiro_token(v(values, "f-h")),
        fonte: fonte.clone(),
        resumo: longo(v(values, "f-resumo"), 500),
        forca: forca_de(v(values, "f-forca")),
        contra: v(values, "f-contra") == "true",
        empresa: linha(v(values, "f-emp"), 80),
        papel: String::new(),
        em,
    };
    // Fonte = id de uma entrevista: herda a empresa (se não informada) e o papel.
    if Col::de_id(&fonte) == Some(Col::I) {
        if let Some(i) = ler::<Entrevista>(&fonte) {
            if e.empresa.is_empty() {
                e.empresa = i.empresa.clone();
            }
            e.papel = i.papel.clone();
        }
    }
    e
}

fn decisao_de(values: &Valores, em: u64) -> Decisao {
    let validas = indice(Col::H);
    let h = values
        .iter()
        .filter(|(_, val)| val.as_str() == "true")
        .filter_map(|(k, _)| k.strip_prefix("hk-"))
        .filter(|id| validas.iter().any(|x| x == id))
        .map(str::to_string)
        .collect();
    Decisao {
        t: primeiro_token(v(values, "f-t")),
        h,
        escolha: codigo(ESCOLHAS, v(values, "f-esc")).to_string(),
        porque: longo(v(values, "f-porque"), 1000),
        proximo: longo(v(values, "f-prox"), 500),
        em,
    }
}

fn salvar_evidencia(
    sel: &str,
    ctx: &mut Ctx,
    values: &Valores,
    novo: bool,
    agora: u64,
) -> Result<(), String> {
    let em = if novo {
        agora
    } else {
        ler::<Evidencia>(sel).map(|e| e.em).unwrap_or(agora)
    };
    let e = evidencia_de(values, em);
    let r = if ler::<Hipotese>(&e.h).is_none() {
        Err("Escolha uma hipótese existente.".to_string())
    } else if e.fonte.is_empty() {
        Err("Informe a fonte (id da entrevista, ex.: i3, ou texto/URL).".to_string())
    } else if e.resumo.is_empty() {
        Err("Resuma o que a evidência mostra.".to_string())
    } else if novo {
        criar(Col::V, &e).map(|_| ())
    } else {
        gravar(sel, &e)
    };
    if r.is_err() {
        ctx.rascunho = Rascunho::V(e);
    }
    r
}

fn salvar_decisao(
    sel: &str,
    ctx: &mut Ctx,
    values: &Valores,
    novo: bool,
    agora: u64,
) -> Result<(), String> {
    let em = if novo {
        agora
    } else {
        ler::<Decisao>(sel).map(|d| d.em).unwrap_or(agora)
    };
    let d = decisao_de(values, em);
    let r = if ler::<Territorio>(&d.t).is_none() {
        Err("Escolha um território existente.".to_string())
    } else if d.porque.is_empty() {
        Err("Explique o porquê da decisão.".to_string())
    } else if novo {
        criar(Col::D, &d).map(|_| ())
    } else {
        gravar(sel, &d)
    };
    if r.is_err() {
        ctx.rascunho = Rascunho::D(d);
    }
    r
}

fn hipotese_tem_evidencia(h: &str) -> bool {
    indice(Col::V)
        .iter()
        .any(|id| ler::<Evidencia>(id).is_some_and(|e| e.h == h))
}

/// Remove a entrevista e as evidências que a citam como fonte (pedido de exclusão: nada fica).
fn remover_entrevista(id: &str) -> Result<(), String> {
    for v_id in indice(Col::V) {
        if ler::<Evidencia>(&v_id).is_some_and(|e| e.fonte == id) {
            apagar(Col::V, &v_id)?;
        }
    }
    apagar(Col::I, id)
}

fn tela_evidencias(u: &Ui) -> Vec<Node> {
    let mut f = vec![ui::text(
        "Evidência: um fato ligado a uma hipótese, com fonte e força. Conta empresas, não falas.",
    )];
    f.extend(lista(
        Col::V,
        u,
        |id| {
            let e: Evidencia = ler(id)?;
            Some(corta(
                &format!(
                    "{id} · {} · F{} · {} · {} · {}",
                    e.h,
                    e.forca,
                    if e.contra { "contra" } else { "a favor" },
                    if e.empresa.is_empty() {
                        "sem empresa"
                    } else {
                        &e.empresa
                    },
                    corta(&e.resumo, 70)
                ),
                200,
            ))
        },
        "Nenhuma evidência ainda.",
        "Nova evidência",
    ));
    f
}

fn tela_decisoes(u: &Ui) -> Vec<Node> {
    let mut f = vec![ui::text(
        "Decisão: avançar, restringir, reformular ou abandonar, com a justificativa e o próximo experimento.",
    )];
    f.extend(lista(
        Col::D,
        u,
        |id| {
            let d: Decisao = ler(id)?;
            Some(corta(
                &format!(
                    "{id} · {} · {} · {}",
                    d.t,
                    rotulo(ESCOLHAS, &d.escolha),
                    corta(&d.porque, 90)
                ),
                200,
            ))
        },
        "Nenhuma decisão ainda.",
        "Nova decisão",
    ));
    f
}

fn opcoes_territorio() -> Vec<(String, String)> {
    indice(Col::T)
        .into_iter()
        .filter_map(|id| {
            let t: Territorio = ler(&id)?;
            Some((id.clone(), format!("{id} · {}", corta(&t.nome, 60))))
        })
        .collect()
}

fn ficha_evidencia(sel: &str, r: &Rascunho) -> Vec<Node> {
    let e = match r {
        Rascunho::V(e) => e.clone(),
        _ => ler(sel).unwrap_or_default(),
    };
    let hs: Vec<(String, String)> = indice(Col::H)
        .into_iter()
        .filter_map(|id| {
            let h: Hipotese = ler(&id)?;
            Some((id.clone(), format!("{id} · {}", corta(&h.texto, 60))))
        })
        .collect();
    if hs.is_empty() {
        return vec![
            ui::text("Crie uma hipótese antes de registrar evidências."),
            ui::button("cancelar", "Voltar"),
        ];
    }
    let escolhida = hs
        .iter()
        .find(|(id, _)| *id == e.h)
        .map(|(_, r)| r.clone())
        .unwrap_or_else(|| hs[0].1.clone());
    vec![
        ui::heading(if sel.is_empty() {
            "Nova evidência"
        } else {
            "Evidência"
        }),
        ui::select(
            "f-h",
            "Hipótese",
            hs.iter().map(|(_, r)| r.clone()),
            escolhida,
        ),
        ui::input(
            "f-fonte",
            "Fonte: id da entrevista (ex.: i3) ou texto/URL",
            e.fonte,
        ),
        ui::textarea("f-resumo", "O que a evidência mostra (até 500)", e.resumo),
        ui::select(
            "f-forca",
            "Força da evidência",
            FORCAS.iter().copied(),
            forca_rotulo(e.forca),
        ),
        ui::checkbox(
            "f-contra",
            "Evidência contrária (contradiz a hipótese)",
            e.contra,
        ),
        ui::input(
            "f-emp",
            "Empresa (vem da entrevista, se a fonte for uma)",
            e.empresa,
        ),
        botoes_ficha(),
    ]
}

fn ficha_decisao(sel: &str, r: &Rascunho) -> Vec<Node> {
    let d = match r {
        Rascunho::D(d) => d.clone(),
        _ => ler(sel).unwrap_or_default(),
    };
    let ts = opcoes_territorio();
    if ts.is_empty() {
        return vec![
            ui::text("Crie um território antes de registrar decisões."),
            ui::button("cancelar", "Voltar"),
        ];
    }
    let escolhido = ts
        .iter()
        .find(|(id, _)| *id == d.t)
        .map(|(_, r)| r.clone())
        .unwrap_or_else(|| ts[0].1.clone());
    let mut f = vec![
        ui::heading(if sel.is_empty() {
            "Nova decisão"
        } else {
            "Decisão"
        }),
        ui::select(
            "f-t",
            "Território",
            ts.iter().map(|(_, r)| r.clone()),
            escolhido,
        ),
        ui::select(
            "f-esc",
            "Decisão",
            rotulos(ESCOLHAS),
            rotulo(ESCOLHAS, &d.escolha),
        ),
        ui::textarea("f-porque", "Por quê (até 1000)", d.porque),
        ui::textarea("f-prox", "Próximo experimento", d.proximo),
    ];
    let hs: Vec<(String, Hipotese)> = indice(Col::H)
        .into_iter()
        .filter_map(|id| Some((id.clone(), ler::<Hipotese>(&id)?)))
        .collect();
    if !hs.is_empty() {
        f.push(ui::text("Hipóteses consideradas nesta decisão:"));
        for (id, h) in hs {
            f.push(ui::checkbox(
                format!("hk-{id}"),
                format!("{id} · {}", corta(&h.texto, 70)),
                d.h.contains(&id),
            ));
        }
    }
    f.push(botoes_ficha());
    f
}

// ---- painel (tudo calculado na hora; nada disso é gravado)

/// Empresa normalizada: minúsculas, sem espaços sobrando.
fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Default)]
struct Aval {
    favor: usize,
    contra: usize,
    empresas: BTreeSet<String>,
    papeis: BTreeSet<String>,
    forca_favor: u8,
    forca_contra: u8,
    ultima_contra: u64,
}

fn avaliar(h: &str, evs: &[Evidencia]) -> Aval {
    let mut a = Aval::default();
    for e in evs.iter().filter(|e| e.h == h) {
        if e.contra {
            a.contra += 1;
            a.forca_contra = a.forca_contra.max(e.forca);
            a.ultima_contra = a.ultima_contra.max(e.em);
        } else {
            a.favor += 1;
            a.forca_favor = a.forca_favor.max(e.forca);
            if !norm(&e.empresa).is_empty() {
                a.empresas.insert(norm(&e.empresa));
            }
            if !e.papel.is_empty() {
                a.papeis.insert(e.papel.clone());
            }
        }
    }
    a
}

/// Há evidência contrária registrada e nenhuma decisão posterior a ela considerou a hipótese.
fn contradicao_aberta(h: &str, a: &Aval, decs: &[Decisao]) -> bool {
    a.contra > 0
        && !decs
            .iter()
            .any(|d| d.em > a.ultima_contra && d.h.iter().any(|x| x == h))
}

fn marca(ok: bool) -> &'static str {
    if ok {
        "✓"
    } else {
        "✗"
    }
}

fn tela_painel() -> Vec<Node> {
    let ts: Vec<(String, Territorio)> = indice(Col::T)
        .into_iter()
        .filter_map(|id| Some((id.clone(), ler(&id)?)))
        .collect();
    let hs: Vec<(String, Hipotese)> = indice(Col::H)
        .into_iter()
        .filter_map(|id| Some((id.clone(), ler(&id)?)))
        .collect();
    let evs: Vec<Evidencia> = indice(Col::V).iter().filter_map(|id| ler(id)).collect();
    let decs: Vec<(String, Decisao)> = indice(Col::D)
        .into_iter()
        .filter_map(|id| Some((id.clone(), ler(&id)?)))
        .collect();
    let decs_so: Vec<Decisao> = decs.iter().map(|(_, d)| d.clone()).collect();
    let mut f = vec![ui::text(
        "Painel calculado do que você registrou. Os critérios são sugestões para decidir, não uma regra: conte empresas independentes, não falas.",
    )];
    if ts.is_empty() {
        f.push(ui::text("Sem territórios ainda."));
        return f;
    }
    for (tid, t) in &ts {
        f.push(ui::heading(corta(&format!("{tid} · {}", t.nome), 120)));
        let hs_t: Vec<&(String, Hipotese)> = hs.iter().filter(|(_, h)| h.t == *tid).collect();
        let mut empresas = BTreeSet::new();
        let (mut max_f, mut aberta) = (0u8, false);
        let mut linhas = Vec::new();
        for (id, h) in &hs_t {
            let a = avaliar(id, &evs);
            let ab = contradicao_aberta(id, &a, &decs_so);
            let mut l = format!(
                "{id} [{}] {} · a favor: {} (empresas indep.: {}; papéis: {}; força máx.: {}) · contra: {} (força máx.: {})",
                rotulo(STATUS, &h.status),
                corta(&h.texto, 50),
                a.favor,
                a.empresas.len(),
                a.papeis.len(),
                a.forca_favor,
                a.contra,
                a.forca_contra
            );
            if ab {
                l.push_str(" · ⚠ contradição aberta");
            }
            if h.contra.is_empty() {
                l.push_str(" · ⚠ sem evidência contrária definida");
            }
            linhas.push(corta(&l, 200));
            empresas.extend(a.empresas.iter().cloned());
            max_f = max_f.max(a.forca_favor);
            aberta |= ab;
        }
        if linhas.is_empty() {
            f.push(ui::text("Sem hipóteses neste território."));
        } else {
            f.push(ui::list(linhas));
        }
        f.push(ui::list(vec![
            format!(
                "{} Segmento identificável (nome e processo preenchidos)",
                marca(!t.nome.is_empty() && !t.processo.is_empty())
            ),
            format!(
                "{} Episódios em empresas independentes: {} (sugestão: 3 ou mais)",
                marca(empresas.len() >= 3),
                empresas.len()
            ),
            format!(
                "{} Alternativa atual compreendida",
                marca(!t.alternativa.is_empty())
            ),
            format!(
                "{} Mobilização para mudar (evidência de força 4+ a favor)",
                marca(max_f >= 4)
            ),
            format!(
                "{} Acesso a quem pode testar (evidência de força 5+)",
                marca(max_f >= 5)
            ),
            format!(
                "{} Contradições revisadas (sem contradição aberta)",
                marca(!aberta)
            ),
            "• Consequências relevantes: confira nas fichas (campo Fato); o painel não mede isso."
                .to_string(),
        ]));
        let ultima = decs
            .iter()
            .filter(|(_, d)| d.t == *tid)
            .max_by_key(|(_, d)| d.em);
        f.push(ui::text(match ultima {
            Some((id, d)) => format!(
                "Última decisão: {} ({id}): {}",
                rotulo(ESCOLHAS, &d.escolha),
                corta(&d.porque, 200)
            ),
            None => "Nenhuma decisão registrada para este território.".to_string(),
        }));
    }
    // Pendências gerais.
    let sem_evidencia: Vec<&str> = hs
        .iter()
        .filter(|(id, _)| !evs.iter().any(|e| e.h == *id))
        .map(|(id, _)| id.as_str())
        .collect();
    let (mut sem_interp, mut sem_duvida) = (0, 0);
    for id in indice(Col::I) {
        if let Some(e) = ler::<Entrevista>(&id) {
            sem_interp += usize::from(e.interpretacao.is_empty());
            sem_duvida += usize::from(e.duvida.is_empty());
        }
    }
    f.push(ui::heading("Pendências"));
    f.push(ui::list(vec![
        format!(
            "Hipóteses sem nenhuma evidência: {}",
            if sem_evidencia.is_empty() {
                "nenhuma".to_string()
            } else {
                corta(&sem_evidencia.join(", "), 150)
            }
        ),
        format!("Entrevistas sem interpretação: {sem_interp}; sem dúvidas anotadas: {sem_duvida}"),
    ]));
    f
}

// ---------------------------------------------------------------- guia para quem está começando

/// Quantas entrevistas formam o primeiro lote sugerido (um número para começar, não uma regra).
const PRIMEIRO_LOTE: usize = 5;

/// Exemplos em cinza dentro dos campos (placeholder, até 80 caracteres), por id de campo.
const EXEMPLOS: &[(&str, &str)] = &[
    ("f-nome", "ex.: Financeiro de pequenas empresas"),
    (
        "f-processo",
        "ex.: fechamento do mês e conciliação bancária",
    ),
    ("f-executor", "ex.: analista financeiro"),
    ("f-responsavel", "ex.: controller ou gerente financeiro"),
    (
        "f-contexto",
        "ex.: empresas de 10 a 50 pessoas, vários bancos",
    ),
    ("f-alternativa", "ex.: planilhas e conferência manual"),
    ("f-gatilho", "ex.: crescimento, auditoria, troca de sistema"),
    (
        "f-texto",
        "ex.: fechar o mês leva mais de 3 dias por conferência manual",
    ),
    (
        "f-procura",
        "ex.: relatos de retrabalho, horas gastas, erros que custaram dinheiro",
    ),
    (
        "f-contra",
        "ex.: ninguém reclama, ou a planilha atual resolve bem",
    ),
    (
        "f-experimento",
        "ex.: entrevistar 5 analistas sobre a última vez que fecharam o mês",
    ),
    ("f-part", "ex.: P1"),
    ("f-emp", "ex.: Acme"),
    ("f-data", "ex.: 2026-11-13"),
    (
        "f-fato",
        "ex.: na última vez levou 3 dias e refizeram a conciliação 2 vezes",
    ),
    (
        "f-interp",
        "ex.: parece que o problema é a conferência, não o sistema",
    ),
    (
        "f-duvida",
        "ex.: isso acontece todo mês? quem aprova o orçamento?",
    ),
    ("f-fonte", "ex.: i3 (a entrevista) ou o link de onde veio"),
    (
        "f-resumo",
        "ex.: levou 3 dias e refez a conciliação duas vezes",
    ),
    (
        "f-porque",
        "ex.: 3 empresas independentes relataram o mesmo problema recente",
    ),
    (
        "f-prox",
        "ex.: fazer a conciliação à mão para 2 clientes, para testar",
    ),
];

/// Coloca os exemplos nos campos de texto (percorre a tela inteira).
fn dicas(n: &mut Node) {
    match n {
        Node::Stack { children } | Node::Row { children } => children.iter_mut().for_each(dicas),
        Node::Input {
            id, placeholder, ..
        }
        | Node::Textarea {
            id, placeholder, ..
        } => {
            if let Some((_, ex)) = EXEMPLOS.iter().find(|(k, _)| k == id) {
                *placeholder = (*ex).to_string();
            }
        }
        _ => {}
    }
}

/// Uma dica curta no alto de cada ficha, dizendo o que escrever.
fn ajuda_form(tela: &str) -> Option<&'static str> {
    Some(match tela {
        "form-t" => "Dica: escolha uma área pequena e concreta (dá para mudar depois). Os exemplos em cinza mostram o formato; só o nome é obrigatório.",
        "form-h" => "Dica: escreva algo que possa estar ERRADO, como \"fechar o mês leva dias por conferência manual\". Depois você procura provas a favor e contra.",
        "form-i" => "Dica: preencha logo depois da conversa. Fato = o que a pessoa contou. Interpretação = o que você acha. Use um código (P1, P2), não o nome. Só o código é obrigatório.",
        "form-v" => "Dica: uma evidência é UM fato com fonte. Vale mais quanto mais a pessoa se comprometeu (tentou resolver, testou, pagou). Marque \"contrária\" se ela desmente a hipótese.",
        "form-d" => "Dica: decida olhando o Painel. Não precisa estar certo: precisa estar escrito, com o motivo e o próximo experimento.",
        _ => return None,
    })
}

fn tela_inicio() -> Vec<Node> {
    let n = |c: Col| indice(c).len();
    let (nt, nh, ni, nv, nd) = (n(Col::T), n(Col::H), n(Col::I), n(Col::V), n(Col::D));
    // (feito?, descrição): o primeiro não feito é o "agora".
    let passos: [(bool, String); 6] = [
        (nt > 0, format!("1. Escolha onde olhar (Territórios): {nt} criado(s)")),
        (nh > 0, format!("2. Escreva o que você acredita (Hipóteses): {nh}")),
        (ni > 0, "3. Leia o Roteiro e marque as conversas".to_string()),
        (
            ni >= PRIMEIRO_LOTE,
            format!("4. Converse e anote cada entrevista (Entrevistas): {ni} de {PRIMEIRO_LOTE} do primeiro lote"),
        ),
        (nv > 0, format!("5. Registre o que provou ou desmentiu (Evidências): {nv}")),
        (nd > 0, format!("6. Olhe o Painel e decida (Decisões): {nd}")),
    ];
    let agora = passos.iter().position(|(feito, _)| !feito);
    let linhas: Vec<String> = passos
        .iter()
        .enumerate()
        .map(|(i, (feito, t))| {
            let marca = if *feito {
                "✓"
            } else if Some(i) == agora {
                "→ AGORA:"
            } else {
                "○"
            };
            format!("{marca} {t}")
        })
        .collect();

    let mut f = vec![
        ui::heading("Por onde começar?"),
        ui::text("Discovery é descobrir, antes de construir, se existe um problema real, para quem e se vale a pena resolvê-lo. Você conversa com pessoas, anota o que ouviu e decide se continua. Aqui você faz isso em seis passos."),
        ui::list(linhas),
        ui::heading("Seu próximo passo"),
    ];
    let (texto, botoes): (String, Vec<Node>) = match agora {
        Some(0) => (
            "Passo 1: crie o primeiro território. É só dar um nome à área que você quer investigar (ex.: \"Financeiro de pequenas empresas\"). Leva um minuto.".into(),
            vec![ui::button("go-form-t", "Criar território")],
        ),
        Some(1) => (
            "Passo 2: escreva sua primeira hipótese: algo que você acredita sobre o problema e que pode estar errado.".into(),
            vec![ui::button("go-form-h", "Escrever hipótese")],
        ),
        Some(2) | Some(3) => (
            format!("Passos 3 e 4: marque conversas com pessoas que vivem o problema e anote cada uma logo depois. Faltam {} entrevista(s) para completar o primeiro lote de {PRIMEIRO_LOTE}. Antes de conversar, leia o Roteiro.", PRIMEIRO_LOTE.saturating_sub(ni)),
            vec![
                ui::button("nav-r", "Ler o roteiro"),
                ui::button("go-form-i", "Registrar entrevista"),
            ],
        ),
        Some(4) => (
            "Passo 5: transforme o que ouviu em evidências: um fato, ligado a uma hipótese, com a fonte. Registre também o que desmente a hipótese.".into(),
            vec![ui::button("go-form-v", "Registrar evidência")],
        ),
        Some(_) => (
            "Passo 6: abra o Painel para ver o que as evidências mostram e registre sua decisão: avançar, restringir, reformular ou abandonar.".into(),
            vec![
                ui::button("go-painel", "Ver o Painel"),
                ui::button("go-form-d", "Registrar decisão"),
            ],
        ),
        None => (
            "Você completou um ciclo! Releia o Painel, escolha o próximo experimento (ele fica na decisão) e volte ao passo 4 com um novo grupo de conversas.".into(),
            vec![ui::button("go-painel", "Ver o Painel")],
        ),
    };
    f.push(ui::text(texto));
    f.push(ui::row(botoes));

    f.push(ui::heading("Quando posso terminar?"));
    f.push(ui::list([
        "Um ciclo termina quando você registra uma decisão (avançar, restringir, reformular ou abandonar) com o motivo e o próximo experimento.",
        "Não existe número mágico. O Painel mostra empresas independentes, evidências fortes e contradições; ele sugere, você decide.",
        "Hipótese refutada é um bom resultado: você descobriu cedo, sem gastar meses construindo a coisa errada.",
        "Depois de decidir, escolha o próximo experimento e volte à fase de conversas com um grupo novo.",
    ]));

    f.push(ui::heading("Palavras que aparecem aqui"));
    f.push(ui::list([
        "Território: a área ou o processo que você vai investigar (ex.: fechamento financeiro).",
        "Hipótese: algo que você acredita e pode ser provado errado.",
        "Entrevista: uma conversa sobre o que a pessoa fez de verdade, não sobre a sua ideia.",
        "Fato × interpretação: fato é o que a pessoa contou; interpretação é o que você conclui disso.",
        "Evidência: um fato com fonte ligado a uma hipótese. A força vai de 1 (reclamação pública) a 7 (pagamento).",
        "Empresa independente: empresas diferentes entre si. Dez falas da mesma empresa contam como uma.",
        "Decisão: o que você faz com o que descobriu, e por quê.",
    ]));

    f.push(ui::heading("Exemplo para se inspirar"));
    f.push(ui::list([
        "Território: Financeiro de pequenas empresas (processo: fechamento do mês).",
        "Hipótese: fechar o mês leva mais de 3 dias por conferência manual.",
        "Entrevista P1 (Acme, responsável): na última vez levou 3 dias e refizeram a conciliação.",
        "Evidência: fonte i3, força 2 (entrevista), a favor da hipótese.",
        "Painel: 3 empresas independentes relataram o mesmo problema.",
        "Decisão: restringir o segmento a empresas com vários bancos; próximo experimento: fazer a conciliação à mão para 2 clientes.",
    ]));
    f
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
            u.tela = "inicio".into();
        }
        if !ctx.somente_leitura {
            // Gravar a tela é conveniência: falha de quota não derruba a execução.
            let _ = gravar("ui", &u);
        }
        Ok(tela(&u, &ctx))
    }
}

export_app!(Discovery);
