//! SDK Rust para mini-apps do Superapp.
//!
//! Um mini-app é uma função de evento: recebe um [`Event`] (abrir, clicar) e devolve a
//! interface como uma árvore [`Node`] de um catálogo fixo (texto, botão, lista, pilha). O
//! host valida a árvore e a desenha; o mini-app nunca produz HTML.
//!
//! ```ignore
//! use superapp_sdk::{export_app, storage, ui, App, Event, Node};
//!
//! struct Contador;
//! impl App for Contador {
//!     fn handle(event: Event) -> Result<Node, String> {
//!         let mut n = storage::get_u64("n").unwrap_or(0);
//!         if matches!(&event, Event::Click { id } if id == "mais") {
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
pub use superapp_ui::{Event, Node};

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
