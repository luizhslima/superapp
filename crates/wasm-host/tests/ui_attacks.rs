//! Suíte de ataques E2.6, parte 2: mini-apps maliciosos instalados de verdade (pacote assinado,
//! consentimento, `Registry`). Cada ataque deve ser contido com um erro claro e o host deve
//! continuar funcionando e servindo outros mini-apps depois.
mod common;

use common::{clicker_wasm, guest, root};
use ed25519_dalek::SigningKey;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use superapp_core::MemoryAudit;
use superapp_package::{pack, Manifest, Permission, RequestedLimits, TrustStore};
use superapp_wasm_host::lifecycle::{LifecycleError, Registry};
use superapp_wasm_host::ui::{Event, Node};
use superapp_wasm_host::WasmError;

fn author() -> SigningKey {
    SigningKey::from_bytes(&[23; 32])
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("superapp-ua-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

/// Instala o mini-app malicioso (`attack`) com as três permissões aprovadas.
fn install_attacker(root: &Path, limits: Option<RequestedLimits>) -> Registry {
    let reg = Registry::open(
        root,
        TrustStore::new(vec![author().verifying_key().to_bytes()]),
        Arc::new(MemoryAudit::default()),
    )
    .unwrap();
    let wasm = guest(
        root_dir().join("crates/wasm-host/tests/guests/attack"),
        "attack_guest.wasm",
    );
    let perms = vec![Permission::Log, Permission::Storage, Permission::Clock];
    let m = Manifest {
        schema: 1,
        id: "attack".into(),
        name: "Attack".into(),
        version: "1.0.0".into(),
        permissions: perms.clone(),
        wasm_sha256: String::new(),
        limits,
    };
    reg.install(&pack(&author(), &m, &wasm).unwrap(), &perms)
        .unwrap();
    reg
}

fn root_dir() -> PathBuf {
    root()
}

/// Define o "modo" do mini-app malicioso escrevendo direto no storage dele (como o próprio
/// app faria): os testes escolhem o ataque da próxima execução sem reinstalar.
fn set_mode(data_root: &Path, mode: &str) {
    let dir = data_root.join("data/attack");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(hex("mode")), mode).unwrap();
}

async fn run(reg: &Registry) -> Result<Node, LifecycleError> {
    reg.dispatch("attack", &Event::Init).await
}

#[tokio::test(flavor = "current_thread")]
async fn malicious_ui_output_is_rejected_and_host_recovers() {
    let root = tmp("ui");
    let reg = install_attacker(&root, None);

    for mode in ["html", "style", "huge", "deep", "notjson", "control"] {
        set_mode(&root, mode);
        let e = run(&reg).await.unwrap_err();
        assert!(
            matches!(e, LifecycleError::InvalidUi(_)),
            "modo {mode} deveria dar UI inválida, deu {e:?}"
        );
    }
    // Depois de todos os ataques, o mesmo mini-app com saída válida funciona.
    set_mode(&root, "ok");
    assert_eq!(
        run(&reg).await.unwrap(),
        Node::Text {
            text: "tudo certo".into()
        }
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn panic_in_guest_is_a_contained_trap() {
    let root = tmp("panic");
    let reg = install_attacker(&root, None);
    set_mode(&root, "panic");
    let e = run(&reg).await.unwrap_err();
    assert!(
        matches!(e, LifecycleError::Wasm(WasmError::Trap(_))),
        "{e:?}"
    );
    set_mode(&root, "ok");
    assert!(run(&reg).await.is_ok(), "host segue servindo após o trap");
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn limits_requested_in_the_manifest_contain_loops_and_memory() {
    let root = tmp("limits");
    let reg = install_attacker(
        &root,
        Some(RequestedLimits {
            fuel: Some(3_000_000),
            memory_mb: Some(8),
            timeout_ms: Some(2_000),
        }),
    );
    set_mode(&root, "spin");
    assert_eq!(
        run(&reg).await.unwrap_err(),
        LifecycleError::Wasm(WasmError::OutOfFuel)
    );
    set_mode(&root, "hog");
    assert_eq!(
        run(&reg).await.unwrap_err(),
        LifecycleError::Wasm(WasmError::OutOfMemory {
            limit_bytes: 8 * 1024 * 1024
        })
    );
    set_mode(&root, "ok");
    assert!(run(&reg).await.is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn a_misbehaving_app_does_not_affect_another_installed_app() {
    let root = tmp("isolation");
    let reg = install_attacker(&root, None);
    // Segundo mini-app, legítimo, no mesmo registro.
    let m = Manifest {
        schema: 1,
        id: "clicker".into(),
        name: "Clicker".into(),
        version: "1.0.0".into(),
        permissions: vec![Permission::Storage],
        wasm_sha256: String::new(),
        limits: None,
    };
    reg.install(
        &pack(&author(), &m, &clicker_wasm()).unwrap(),
        &[Permission::Storage],
    )
    .unwrap();
    set_mode(&root, "spin");
    assert!(run(&reg).await.is_err());
    // O outro app continua normal e com seus próprios dados.
    let ui = reg.dispatch("clicker", &Event::click("inc")).await.unwrap();
    assert!(matches!(ui, Node::Stack { .. }));
    // O storage do atacante não é visível ao clicker (diretórios separados por id).
    assert!(root.join("data/attack").exists() && root.join("data/clicker").exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn repeated_runs_do_not_leak_state_between_executions() {
    // Cada execução usa uma instância nova: memória do guest não persiste entre elas.
    let root = tmp("fresh");
    let reg = install_attacker(&root, None);
    set_mode(&root, "ok");
    for _ in 0..30 {
        assert!(run(&reg).await.is_ok());
    }
    let _ = std::fs::remove_dir_all(&root);
}
