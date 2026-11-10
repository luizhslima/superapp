//! Mini-app de exemplo com UI declarativa (ADR-009): mostra um contador e um botão.
//! Recebe eventos em JSON (`{"type":"init"}`, `{"type":"click","id":"inc"}`) e devolve a
//! árvore de UI em JSON. Só precisa da permissão `storage`.
wit_bindgen::generate!({ world: "miniapp", path: "../../crates/wasm-host/wit" });

use superapp::host::storage;

struct Clicker;

impl Guest for Clicker {
    fn run(input: String) -> Result<String, String> {
        let mut n = storage::get("clicks")
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        if input.contains("\"click\"") && input.contains("\"inc\"") {
            n += 1;
            storage::set("clicks", n.to_string().as_bytes())?;
        }
        Ok(format!(
            r#"{{"type":"stack","children":[{{"type":"text","text":"Cliques: {n}"}},{{"type":"button","id":"inc","label":"Incrementar"}}]}}"#
        ))
    }
}

export!(Clicker);
