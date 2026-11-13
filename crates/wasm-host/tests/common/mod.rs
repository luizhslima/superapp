//! Helpers compartilhados pelos testes de integração: compila os guests de exemplo e de ataque.
#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Compila os guests para wasm32-wasip2 (uma vez por processo) e devolve os bytes de um deles.
pub fn guest(dir: PathBuf, file: &str) -> Vec<u8> {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        for d in [
            root().join("crates/wasm-host/tests/guests/attack"),
            root().join("miniapps/counter"),
            root().join("miniapps/clicker"),
            root().join("miniapps/tarefas"),
            root().join("miniapps/discovery"),
            root().join("crates/wasm-host/tests/guests/net"),
        ] {
            let status = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
                .args(["build", "--release", "--target", "wasm32-wasip2"])
                .current_dir(&d)
                .status()
                .expect("falha ao executar cargo para compilar o guest");
            assert!(
                status.success(),
                "guest {} não compilou (rustup target add wasm32-wasip2)",
                d.display()
            );
        }
    });
    std::fs::read(dir.join("target/wasm32-wasip2/release").join(file))
        .expect("wasm do guest não encontrado")
}

pub fn tarefas_wasm() -> Vec<u8> {
    guest(root().join("miniapps/tarefas"), "tarefas.wasm")
}

pub fn clicker_wasm() -> Vec<u8> {
    guest(root().join("miniapps/clicker"), "clicker.wasm")
}

pub fn discovery_wasm() -> Vec<u8> {
    guest(root().join("miniapps/discovery"), "discovery.wasm")
}

pub fn net_wasm() -> Vec<u8> {
    guest(
        root().join("crates/wasm-host/tests/guests/net"),
        "net_guest.wasm",
    )
}
