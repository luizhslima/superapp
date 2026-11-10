//! UI declarativa de mini-apps (ADR-009): o app devolve uma árvore de um catálogo fixo; o
//! shell a desenha só com `textContent`. Nada de HTML, estilo ou script vindo do mini-app.
//!
//! Protocolo sobre o `run(input) -> output` do WIT v0:
//! - entrada: um [`Event`] em JSON (`{"type":"init"}` ou `{"type":"click","id":"inc"}`);
//! - saída: uma [`Node`] raiz em JSON.
use serde::{Deserialize, Serialize};

pub const MAX_DEPTH: usize = 8;
pub const MAX_NODES: usize = 200;
pub const MAX_TEXT_CHARS: usize = 500;
pub const MAX_LIST_ITEMS: usize = 100;
pub const MAX_ITEM_CHARS: usize = 200;
/// Teto do JSON de saída antes mesmo de interpretar.
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Evento enviado ao mini-app. Montado pelo host; o `id` vindo da UI é validado antes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Init,
    Click { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Node {
    Text { text: String },
    Button { id: String, label: String },
    List { items: Vec<String> },
    Stack { children: Vec<Node> },
}

/// Id de elemento interativo: `[a-z0-9_-]{1,32}`.
pub fn valid_element_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn text_ok(s: &str, max: usize) -> bool {
    s.chars().count() <= max && !s.chars().any(|c| c.is_control() && c != '\n')
}

impl Event {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Event::Init => Ok(()),
            Event::Click { id } if valid_element_id(id) => Ok(()),
            Event::Click { .. } => Err("id de elemento inválido".into()),
        }
    }
}

impl Node {
    /// Interpreta e valida a saída de um mini-app (não confiável).
    pub fn parse(output: &str) -> Result<Node, String> {
        if output.len() > MAX_OUTPUT_BYTES {
            return Err("saída da UI grande demais".into());
        }
        let node: Node = serde_json::from_str(output).map_err(|e| format!("UI inválida: {e}"))?;
        let mut count = 0;
        node.check(1, &mut count)?;
        Ok(node)
    }

    fn check(&self, depth: usize, count: &mut usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("UI profunda demais".into());
        }
        *count += 1;
        if *count > MAX_NODES {
            return Err("UI com nós demais".into());
        }
        match self {
            Node::Text { text } if text_ok(text, MAX_TEXT_CHARS) => Ok(()),
            Node::Text { .. } => Err("texto inválido".into()),
            Node::Button { id, label } => {
                if valid_element_id(id) && !label.is_empty() && text_ok(label, 80) {
                    Ok(())
                } else {
                    Err("botão inválido".into())
                }
            }
            Node::List { items } => {
                if items.len() <= MAX_LIST_ITEMS && items.iter().all(|i| text_ok(i, MAX_ITEM_CHARS))
                {
                    Ok(())
                } else {
                    Err("lista inválida".into())
                }
            }
            Node::Stack { children } => children.iter().try_for_each(|c| c.check(depth + 1, count)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_tree_parses() {
        let n = Node::parse(
            r#"{"type":"stack","children":[{"type":"text","text":"oi"},{"type":"button","id":"inc","label":"+"},{"type":"list","items":["a","b"]}]}"#,
        )
        .unwrap();
        assert!(matches!(n, Node::Stack { children } if children.len() == 3));
    }

    #[test]
    fn rejects_html_like_unknown_types_and_extra_fields() {
        for bad in [
            r#"{"type":"html","html":"<script>alert(1)</script>"}"#,
            r#"{"type":"text","text":"x","style":"color:red"}"#,
            r#"{"type":"button","id":"Inc Rement","label":"x"}"#,
            r#"{"type":"button","id":"ok","label":""}"#,
            r#"{"type":"text","text":"a\u0000b"}"#,
            "not json",
        ] {
            assert!(Node::parse(bad).is_err(), "deveria recusar: {bad}");
        }
    }

    #[test]
    fn enforces_depth_node_and_size_limits() {
        let mut deep = r#"{"type":"text","text":"x"}"#.to_string();
        for _ in 0..MAX_DEPTH {
            deep = format!(r#"{{"type":"stack","children":[{deep}]}}"#);
        }
        assert!(Node::parse(&deep).is_err());
        let many = format!(
            r#"{{"type":"stack","children":[{}]}}"#,
            vec![r#"{"type":"text","text":"x"}"#; MAX_NODES].join(",")
        );
        assert!(Node::parse(&many).is_err());
        let big = format!(
            r#"{{"type":"text","text":"{}"}}"#,
            "x".repeat(MAX_OUTPUT_BYTES)
        );
        assert!(Node::parse(&big).is_err());
        let long = format!(
            r#"{{"type":"text","text":"{}"}}"#,
            "x".repeat(MAX_TEXT_CHARS + 1)
        );
        assert!(Node::parse(&long).is_err());
    }

    #[test]
    fn event_ids_are_validated() {
        assert!(Event::Click { id: "inc".into() }.validate().is_ok());
        assert!(Event::Click { id: "../x".into() }.validate().is_err());
        assert!(Event::Click { id: String::new() }.validate().is_err());
    }
}
