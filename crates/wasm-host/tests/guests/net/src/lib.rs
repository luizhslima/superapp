//! Guest de teste da rede mediada (ADR-011). Cada clique faz chamadas `http` e mostra o resultado
//! como texto, para os testes conferirem sem depender de nada além da UI declarativa.
use superapp_sdk::{export_app, http, ui, App, Event, Node};

struct Net;

fn resumo(r: Result<http::Response, String>) -> String {
    match r {
        Ok(r) => format!(
            "ok status={} bytes={} tipo={} corpo={}",
            r.status,
            r.body.len(),
            r.content_type,
            String::from_utf8_lossy(&r.body).chars().take(60).collect::<String>()
        ),
        Err(e) => format!("erro {e}"),
    }
}

impl App for Net {
    fn handle(event: Event) -> Result<Node, String> {
        let Event::Click { id, values } = event else {
            return Ok(ui::text("pronto"));
        };
        let url = values.get("url").cloned().unwrap_or_default();
        let linhas = match id.as_str() {
            "get" => vec![resumo(http::get(&url))],
            "post" => vec![resumo(http::post(&url, "application/json", b"{\"a\":1}"))],
            // Seis chamadas seguidas: a quinta e a sexta passam do teto por execução.
            "muitas" => (0..6).map(|_| resumo(http::get(&url))).collect(),
            "cabecalho" => vec![resumo(http::fetch(
                "GET",
                &url,
                &[("Authorization", "Bearer segredo")],
                &[],
            ))],
            _ => vec!["?".into()],
        };
        Ok(ui::stack(linhas.into_iter().map(ui::text).collect()))
    }
}

export_app!(Net);
