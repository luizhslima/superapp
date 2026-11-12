//! SDK Rust para mini-apps do Superapp.
//!
//! Um mini-app é uma função de evento: recebe um [`Event`] (abrir, clicar) e devolve a
//! interface como uma árvore [`Node`] de um catálogo fixo (texto, título, botão, lista, campos de entrada, pilha e linha). O
//! host valida a árvore e a desenha; o mini-app nunca produz HTML.
//!
//! ```ignore
//! use superapp_sdk::{export_app, storage, ui, App, Event, Node};
//!
//! struct Contador;
//! impl App for Contador {
//!     fn handle(event: Event) -> Result<Node, String> {
//!         let mut n = storage::get_u64("n").unwrap_or(0);
//!         if matches!(&event, Event::Click { id, .. } if id == "mais") {
//!             n += 1;
//!             storage::set_u64("n", n)?;
//!         }
//!         Ok(ui::stack(vec![ui::text(format!("Valor: {n}")), ui::button("mais", "+1")]))
//!     }
//! }
//! export_app!(Contador);
//! ```
//!
//! Permissões: só as interfaces que o código realmente usa viram importações do componente
//! (`storage` ao chamar [`storage`], `log` ao chamar [`log`], `clock` ao chamar [`now_ms`]).
//! Declare exatamente essas no manifesto.

/// Bindings gerados do WIT do host (`crates/wasm-host/wit`). Detalhe de implementação.
#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        world: "miniapp",
        path: "../../crates/wasm-host/wit",
        pub_export_macro: true,
        export_macro_name: "export_miniapp",
        default_bindings_module: "superapp_sdk::bindings",
    });
}

pub use serde_json;
pub use superapp_ui::{Action, ActionItem, Event, Node};

/// Contrato de um mini-app.
pub trait App {
    fn handle(event: Event) -> Result<Node, String>;
}

/// Mensagem de diagnóstico (permissão `log`). O host limita linhas e tamanho.
pub fn log(msg: &str) {
    bindings::superapp::host::log::log(msg);
}

/// Milissegundos desde a época Unix (permissão `clock`).
pub fn now_ms() -> u64 {
    bindings::superapp::host::clock::now_ms()
}

/// Construtores da UI declarativa.
pub mod ui {
    use super::Node;

    pub fn text(s: impl Into<String>) -> Node {
        Node::Text { text: s.into() }
    }
    pub fn button(id: impl Into<String>, label: impl Into<String>) -> Node {
        Node::Button {
            id: id.into(),
            label: label.into(),
        }
    }
    pub fn list<S: Into<String>>(items: impl IntoIterator<Item = S>) -> Node {
        Node::List {
            items: items.into_iter().map(Into::into).collect(),
        }
    }
    pub fn stack(children: Vec<Node>) -> Node {
        Node::Stack { children }
    }
    pub fn row(children: Vec<Node>) -> Node {
        Node::Row { children }
    }
    pub fn heading(s: impl Into<String>) -> Node {
        Node::Heading { text: s.into() }
    }
    pub fn divider() -> Node {
        Node::Divider
    }
    /// Campo de texto. Na próxima `Event::Click`, o valor digitado vem em `values[id]`.
    pub fn input(id: impl Into<String>, label: impl Into<String>, value: impl Into<String>) -> Node {
        Node::Input {
            id: id.into(),
            label: label.into(),
            value: value.into(),
            placeholder: String::new(),
        }
    }
    pub fn checkbox(id: impl Into<String>, label: impl Into<String>, checked: bool) -> Node {
        Node::Checkbox {
            id: id.into(),
            label: label.into(),
            checked,
        }
    }
    pub fn select<S: Into<String>>(
        id: impl Into<String>,
        label: impl Into<String>,
        options: impl IntoIterator<Item = S>,
        selected: impl Into<String>,
    ) -> Node {
        Node::Select {
            id: id.into(),
            label: label.into(),
            options: options.into_iter().map(Into::into).collect(),
            selected: selected.into(),
        }
    }
    pub fn progress(value: u8, label: impl Into<String>) -> Node {
        Node::Progress {
            value,
            label: label.into(),
        }
    }
    /// Lista com botões por item. `done: Some(_)` desenha o texto como concluído/aberto.
    pub fn action_list(items: Vec<super::ActionItem>) -> Node {
        Node::ActionList { items }
    }
    pub fn item(text: impl Into<String>, done: Option<bool>, actions: &[(&str, &str)]) -> super::ActionItem {
        super::ActionItem {
            text: text.into(),
            done,
            actions: actions
                .iter()
                .map(|(id, label)| super::Action {
                    id: (*id).into(),
                    label: (*label).into(),
                })
                .collect(),
        }
    }
}

/// Armazenamento próprio do mini-app (permissão `storage`; quota de 4 MiB no host).
pub mod storage {
    use crate::bindings::superapp::host::storage as raw;

    pub fn get(key: &str) -> Option<Vec<u8>> {
        raw::get(key)
    }
    pub fn set(key: &str, value: &[u8]) -> Result<(), String> {
        raw::set(key, value)
    }
    pub fn delete(key: &str) -> Result<(), String> {
        raw::delete(key)
    }
    pub fn get_u64(key: &str) -> Option<u64> {
        get(key)
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.parse().ok())
    }
    pub fn set_u64(key: &str, value: u64) -> Result<(), String> {
        set(key, value.to_string().as_bytes())
    }
}

/// Liga um tipo que implementa [`App`] ao ponto de entrada do componente.
#[macro_export]
macro_rules! export_app {
    ($app:ty) => {
        struct __SdkEntry;
        impl $crate::bindings::Guest for __SdkEntry {
            fn run(input: ::std::string::String) -> ::std::result::Result<::std::string::String, ::std::string::String> {
                let event: $crate::Event = $crate::serde_json::from_str(&input)
                    .map_err(|e| ::std::format!("evento inválido: {e}"))?;
                let node = <$app as $crate::App>::handle(event)?;
                $crate::serde_json::to_string(&node).map_err(|e| e.to_string())
            }
        }
        $crate::bindings::export_miniapp!(__SdkEntry with_types_in $crate::bindings);
    };
}
