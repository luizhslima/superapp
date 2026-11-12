//! UI declarativa de mini-apps (ADR-009): o app devolve uma árvore de um catálogo fixo; o
//! shell a desenha só com `textContent`. Nada de HTML, estilo ou script vindo do mini-app.
//!
//! Protocolo sobre o `run(input) -> output` do WIT v0:
//! - entrada: um [`Event`] em JSON (`{"type":"init"}` ou `{"type":"click","id":"inc"}`); o clique
//!   leva também os valores atuais dos campos da tela (`"values":{"campo":"texto"}`);
//! - saída: uma [`Node`] raiz em JSON.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub const MAX_DEPTH: usize = 8;
pub const MAX_NODES: usize = 200;
pub const MAX_TEXT_CHARS: usize = 500;
pub const MAX_LIST_ITEMS: usize = 100;
pub const MAX_ITEM_CHARS: usize = 200;
/// Campos de entrada (texto, caixa, seleção) por tela, e valores por evento.
pub const MAX_INPUTS: usize = 32;
/// Tamanho máximo do valor de um campo de texto (inicial ou digitado).
pub const MAX_VALUE_CHARS: usize = 1000;
pub const MAX_SELECT_OPTIONS: usize = 50;
pub const MAX_ACTIONS_PER_ITEM: usize = 4;
/// Teto do JSON de saída antes mesmo de interpretar.
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Evento enviado ao mini-app. Montado pelo host; o `id` vindo da UI é validado antes.
///
/// `Click` leva junto os valores atuais dos campos da tela (modelo de formulário): a UI não
/// reenvia nada a cada tecla, só quando o usuário aciona um botão.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Init,
    Click {
        id: String,
        /// `id` do campo -> valor (texto; caixa: "true"/"false"; seleção: a opção escolhida).
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        values: BTreeMap<String, String>,
    },
}

impl Event {
    /// Clique sem valores de campos.
    pub fn click(id: impl Into<String>) -> Event {
        Event::Click {
            id: id.into(),
            values: BTreeMap::new(),
        }
    }
}

/// Ação de um item de [`Node::ActionList`] (vira um botão que dispara `Click { id }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub id: String,
    pub label: String,
}

/// Item de lista com ações. `done` desenha o texto como concluído (`None` = sem estado).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionItem {
    pub text: String,
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Node {
    Text {
        text: String,
    },
    Heading {
        text: String,
    },
    Divider,
    Button {
        id: String,
        label: String,
    },
    List {
        items: Vec<String>,
    },
    /// Lista em que cada item pode ter botões (marcar, remover...).
    ActionList {
        items: Vec<ActionItem>,
    },
    Stack {
        children: Vec<Node>,
    },
    /// Filhos lado a lado.
    Row {
        children: Vec<Node>,
    },
    Input {
        id: String,
        label: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
    },
    Checkbox {
        id: String,
        label: String,
        #[serde(default)]
        checked: bool,
    },
    Select {
        id: String,
        label: String,
        options: Vec<String>,
        /// Opção escolhida (vazio = a primeira).
        #[serde(default)]
        selected: String,
    },
    /// Progresso de 0 a 100.
    Progress {
        value: u8,
        #[serde(default)]
        label: String,
    },
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

/// Texto de uma linha só (sem quebra de linha).
fn line_ok(s: &str, max: usize) -> bool {
    text_ok(s, max) && !s.contains('\n')
}

impl Event {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Event::Init => Ok(()),
            Event::Click { id, values } => {
                if !valid_element_id(id) {
                    return Err("id de elemento inválido".into());
                }
                if values.len() > MAX_INPUTS {
                    return Err("valores demais".into());
                }
                for (k, v) in values {
                    if !valid_element_id(k) || !text_ok(v, MAX_VALUE_CHARS) {
                        return Err("valor de campo inválido".into());
                    }
                }
                Ok(())
            }
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
        let mut inputs = HashSet::new();
        node.check(1, &mut count, &mut inputs)?;
        Ok(node)
    }

    fn check(
        &self,
        depth: usize,
        count: &mut usize,
        inputs: &mut HashSet<String>,
    ) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("UI profunda demais".into());
        }
        *count += 1;
        if *count > MAX_NODES {
            return Err("UI com nós demais".into());
        }
        // Campos de entrada: id válido, único na tela e dentro do teto.
        let mut input_id = |id: &str| -> Result<(), String> {
            if !valid_element_id(id) {
                return Err("id de campo inválido".into());
            }
            if inputs.len() >= MAX_INPUTS || !inputs.insert(id.to_string()) {
                return Err("campos demais ou id de campo repetido".into());
            }
            Ok(())
        };
        match self {
            Node::Text { text } if text_ok(text, MAX_TEXT_CHARS) => Ok(()),
            Node::Text { .. } => Err("texto inválido".into()),
            Node::Heading { text } => {
                if !text.is_empty() && line_ok(text, 120) {
                    Ok(())
                } else {
                    Err("título inválido".into())
                }
            }
            Node::Divider => Ok(()),
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
            Node::ActionList { items } => {
                if items.len() > MAX_LIST_ITEMS {
                    return Err("lista inválida".into());
                }
                for item in items {
                    if !text_ok(&item.text, MAX_ITEM_CHARS)
                        || item.actions.len() > MAX_ACTIONS_PER_ITEM
                    {
                        return Err("item de lista inválido".into());
                    }
                    for a in &item.actions {
                        if !valid_element_id(&a.id) || a.label.is_empty() || !line_ok(&a.label, 40)
                        {
                            return Err("ação inválida".into());
                        }
                    }
                    // Cada item e cada ação contam no orçamento de nós.
                    *count += 1 + item.actions.len();
                }
                if *count > MAX_NODES {
                    return Err("UI com nós demais".into());
                }
                Ok(())
            }
            Node::Stack { children } | Node::Row { children } => children
                .iter()
                .try_for_each(|c| c.check(depth + 1, count, inputs)),
            Node::Input {
                id,
                label,
                value,
                placeholder,
            } => {
                input_id(id)?;
                if line_ok(label, 80) && text_ok(value, MAX_VALUE_CHARS) && line_ok(placeholder, 80)
                {
                    Ok(())
                } else {
                    Err("campo de texto inválido".into())
                }
            }
            Node::Checkbox { id, label, .. } => {
                input_id(id)?;
                if !label.is_empty() && line_ok(label, 80) {
                    Ok(())
                } else {
                    Err("caixa inválida".into())
                }
            }
            Node::Select {
                id,
                label,
                options,
                selected,
            } => {
                input_id(id)?;
                let opts_ok = !options.is_empty()
                    && options.len() <= MAX_SELECT_OPTIONS
                    && options.iter().all(|o| !o.is_empty() && line_ok(o, 80));
                if opts_ok
                    && line_ok(label, 80)
                    && (selected.is_empty() || options.contains(selected))
                {
                    Ok(())
                } else {
                    Err("seleção inválida".into())
                }
            }
            Node::Progress { value, label } => {
                if *value <= 100 && line_ok(label, 80) {
                    Ok(())
                } else {
                    Err("progresso inválido".into())
                }
            }
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
        assert!(Event::click("inc").validate().is_ok());
        assert!(Event::click("../x").validate().is_err());
        assert!(Event::click(String::new()).validate().is_err());
    }

    #[test]
    fn events_with_unknown_fields_are_refused() {
        assert!(serde_json::from_str::<Event>(r#"{"type":"click","id":"a","x":1}"#).is_err());
        // (O serde não aplica isso a variantes sem campos, como `Init`: não carrega dado algum.)
    }

    #[test]
    fn old_click_json_without_values_still_parses() {
        let e: Event = serde_json::from_str(r#"{"type":"click","id":"inc"}"#).unwrap();
        assert_eq!(e, Event::click("inc"));
        // Sem valores, o JSON enviado ao mini-app continua idêntico ao formato antigo.
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"type":"click","id":"inc"}"#
        );
    }

    fn click_with(k: &str, v: String) -> Event {
        let mut e = Event::click("add");
        if let Event::Click { values, .. } = &mut e {
            values.insert(k.into(), v);
        }
        e
    }

    #[test]
    fn event_values_are_validated() {
        assert!(click_with("titulo", "comprar pão\ne leite".into())
            .validate()
            .is_ok());
        for (k, v) in [
            ("Titulo", "x".to_string()),
            ("a b", "x".to_string()),
            ("titulo", "a\u{0}b".to_string()),
            ("titulo", "x".repeat(MAX_VALUE_CHARS + 1)),
        ] {
            assert!(
                click_with(k, v).validate().is_err(),
                "deveria recusar chave {k:?}"
            );
        }
        let mut many = Event::click("add");
        if let Event::Click { values, .. } = &mut many {
            for i in 0..=MAX_INPUTS {
                values.insert(format!("c{i}"), "x".into());
            }
        }
        assert!(many.validate().is_err());
    }

    #[test]
    fn new_components_parse() {
        let n = Node::parse(
            r#"{"type":"stack","children":[
              {"type":"heading","text":"Tarefas"},
              {"type":"row","children":[
                {"type":"input","id":"nova","label":"Nova tarefa","placeholder":"o que fazer?"},
                {"type":"button","id":"add","label":"Adicionar"}]},
              {"type":"checkbox","id":"so_abertas","label":"Só abertas","checked":true},
              {"type":"select","id":"ordem","label":"Ordem","options":["data","nome"],"selected":"nome"},
              {"type":"divider"},
              {"type":"action_list","items":[
                {"text":"pão","done":false,"actions":[{"id":"ok-1","label":"Feito"},{"id":"rm-1","label":"Remover"}]},
                {"text":"leite"}]},
              {"type":"progress","value":40,"label":"1 de 2"}]}"#,
        );
        assert!(n.is_ok(), "{n:?}");
    }

    #[test]
    fn rejects_bad_new_components() {
        for bad in [
            // ids de campo repetidos
            r#"{"type":"stack","children":[{"type":"input","id":"a","label":"x"},{"type":"checkbox","id":"a","label":"y"}]}"#,
            // id inválido
            r#"{"type":"input","id":"A b","label":"x"}"#,
            // seleção com opção escolhida fora da lista, ou sem opções
            r#"{"type":"select","id":"s","label":"x","options":["a"],"selected":"b"}"#,
            r#"{"type":"select","id":"s","label":"x","options":[]}"#,
            // progresso acima de 100
            r#"{"type":"progress","value":101}"#,
            // campos extras (estilo, html)
            r#"{"type":"input","id":"a","label":"x","style":"color:red"}"#,
            r#"{"type":"action_list","items":[{"text":"x","html":"<b>"}]}"#,
            // ação sem rótulo / id ruim / ações demais
            r#"{"type":"action_list","items":[{"text":"x","actions":[{"id":"ok","label":""}]}]}"#,
            r#"{"type":"action_list","items":[{"text":"x","actions":[{"id":"../","label":"a"}]}]}"#,
            r#"{"type":"action_list","items":[{"text":"x","actions":[{"id":"a","label":"1"},{"id":"b","label":"1"},{"id":"c","label":"1"},{"id":"d","label":"1"},{"id":"e","label":"1"}]}]}"#,
            // título vazio ou com quebra de linha
            r#"{"type":"heading","text":""}"#,
            r#"{"type":"heading","text":"a\nb"}"#,
        ] {
            assert!(Node::parse(bad).is_err(), "deveria recusar: {bad}");
        }
    }

    #[test]
    fn input_count_is_capped() {
        let inputs: Vec<String> = (0..=MAX_INPUTS)
            .map(|i| format!(r#"{{"type":"input","id":"c{i}","label":"x"}}"#))
            .collect();
        let tree = format!(r#"{{"type":"stack","children":[{}]}}"#, inputs.join(","));
        assert!(Node::parse(&tree).is_err());
    }

    #[test]
    fn action_lists_count_against_the_node_budget() {
        let item = r#"{"text":"x","actions":[{"id":"a","label":"1"},{"id":"b","label":"2"}]}"#;
        let items = vec![item; 70].join(","); // 70 * (1 + 2) = 210 > MAX_NODES
        let tree = format!(r#"{{"type":"action_list","items":[{items}]}}"#);
        assert!(Node::parse(&tree).is_err());
    }
}
