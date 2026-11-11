//! Segundo idioma (E2.5): mini-app em JavaScript compilado com `jco componentize`.
//! Prova que o contrato WIT é poliglota e mede o custo de um runtime embutido.
//!
//! O componente (`miniapps/clicker-js/clicker-js.wasm`, ~11 MB) é gerado por
//! `node miniapps/clicker-js/build.mjs`. Se ele não existir, os testes só passam quando
//! `SUPERAPP_REQUIRE_JS` não está definido (desenvolvimento local); a CI define a variável,
//! e então a ausência é falha, não "pulado em silêncio".
mod common;

use common::root;
use ed25519_dalek::SigningKey;
use std::sync::Arc;
use std::time::Instant;
use superapp_core::MemoryAudit;
use superapp_package::{pack, verify, Manifest, Permission, TrustStore};
use superapp_wasm_host::lifecycle::Registry;
use superapp_wasm_host::ui::{Event, Node};
use superapp_wasm_host::{Grants, Interface, Limits, MemoryStorage, Runtime, WasmError};

fn js_wasm() -> Option<Vec<u8>> {
    let p = root().join("miniapps/clicker-js/clicker-js.wasm");
    match std::fs::read(&p) {
        Ok(b) => Some(b),
        Err(_) if std::env::var_os("SUPERAPP_REQUIRE_JS").is_none() => {
            eprintln!(
                "AVISO: {} ausente; teste do guest JS pulado (rode build.mjs)",
                p.display()
            );
            None
        }
        Err(e) => panic!("guest JS obrigatório e ausente: {e}"),
    }
}

fn author() -> SigningKey {
    SigningKey::from_bytes(&[31; 32])
}

fn manifest() -> Manifest {
    Manifest {
        schema: 1,
        id: "clicker-js".into(),
        name: "Clicker (JavaScript)".into(),
        version: "0.1.0".into(),
        permissions: vec![Permission::Storage],
        wasm_sha256: String::new(),
        // Sem limites customizados: os padrões (50 M de fuel, 16 MiB, 5 s) bastam, como medido.
        limits: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn js_component_needs_only_storage_and_its_cost_is_measured() {
    let Some(wasm) = js_wasm() else { return };
    let rt = Runtime::new(Arc::new(MemoryAudit::default())).unwrap();

    // Sem `storage` não carrega: a permissão é exigida de verdade.
    let none = rt.load(
        "clicker-js",
        &wasm,
        &Grants::none(),
        Arc::new(MemoryStorage::default()),
    );
    assert!(matches!(none, Err(WasmError::ImportNotGranted(_))));

    // Com o mundo enxuto do build.mjs, só `storage` basta (log e clock não são exigidos).
    let t0 = Instant::now();
    let app = rt
        .load(
            "clicker-js",
            &wasm,
            &Grants::none().with(Interface::Storage),
            Arc::new(MemoryStorage::default()),
        )
        .expect("o componente JS deve carregar só com storage");
    eprintln!(
        "OBS: compilar+ligar o componente JS (11 MB): {:?}",
        t0.elapsed()
    );

    // Com os limites padrão (50 M de fuel, 16 MiB) um guest JS funciona?
    let default = app.run(r#"{"type":"init"}"#, &Limits::default()).await;
    eprintln!(
        "OBS: JS com limites PADRÃO: {}",
        match &default {
            Ok(o) => format!("ok, fuel {}", o.fuel_used),
            Err(e) => format!("falha: {e}"),
        }
    );

    let t1 = Instant::now();
    let out = default.expect("com limites padrão o guest JS deve funcionar");
    eprintln!("OBS: JS por execução: {} de fuel", out.fuel_used);
    let _ = t1;
    assert!(out.output.contains("Cliques (JS)"));
    // A saída passa pelo mesmo validador de UI de qualquer mini-app.
    assert!(Node::parse(&out.output).is_ok());

    // Um pacote assinado com esse wasm é verificado como qualquer outro.
    let pkg = pack(&author(), &manifest(), &wasm).unwrap();
    assert!(verify(
        &pkg,
        &TrustStore::new(vec![author().verifying_key().to_bytes()])
    )
    .is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn js_miniapp_installs_runs_and_persists_through_the_same_host() {
    let Some(wasm) = js_wasm() else { return };
    let root_dir = std::env::temp_dir().join(format!("superapp-js-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root_dir);
    let reg = Registry::open(
        &root_dir,
        TrustStore::new(vec![author().verifying_key().to_bytes()]),
        Arc::new(MemoryAudit::default()),
    )
    .unwrap();
    let bytes = pack(&author(), &manifest(), &wasm).unwrap();
    // O consentimento mostra só `storage`: nada de log nem clock sem necessidade.
    let pv = reg.inspect(&bytes).unwrap();
    assert_eq!(pv.requested, vec![Permission::Storage]);
    reg.install(&bytes, &[Permission::Storage]).unwrap();

    let text = |n: &Node| match n {
        Node::Stack { children } => match &children[0] {
            Node::Text { text } => text.clone(),
            o => panic!("{o:?}"),
        },
        o => panic!("{o:?}"),
    };
    let t0 = Instant::now();
    let first = reg.dispatch("clicker-js", &Event::Init).await.unwrap();
    eprintln!(
        "OBS: JS primeira execução (compila+instancia): {:?}",
        t0.elapsed()
    );
    assert_eq!(text(&first), "Cliques (JS): 0");
    reg.dispatch("clicker-js", &Event::Click { id: "inc".into() })
        .await
        .unwrap();
    let t1 = Instant::now();
    let ui = reg
        .dispatch("clicker-js", &Event::Click { id: "inc".into() })
        .await
        .unwrap();
    eprintln!(
        "OBS: JS execução seguinte (componente já carregado): {:?}",
        t1.elapsed()
    );
    assert_eq!(text(&ui), "Cliques (JS): 2");
    let _ = std::fs::remove_dir_all(&root_dir);
}
