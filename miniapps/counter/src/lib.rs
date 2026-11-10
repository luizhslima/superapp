//! Mini-app de exemplo: conta quantas vezes foi executado e guarda o total em `storage`.
//! Só usa as interfaces `log` e `storage`, então só essas permissões são exigidas.
wit_bindgen::generate!({ world: "miniapp", path: "../../crates/wasm-host/wit" });

use superapp::host::{log::log as host_log, storage};

struct Counter;

impl Guest for Counter {
    fn run(_input: String) -> Result<String, String> {
        let n = storage::get("count")
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
            + 1;
        storage::set("count", n.to_string().as_bytes())?;
        host_log(&format!("execução nº {n}"));
        Ok(format!("execuções: {n}"))
    }
}

export!(Counter);
