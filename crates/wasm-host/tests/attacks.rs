//! Suíte de ataques do host Wasm (E2.6, parte 1): cada ataque deve ser contido e o host
//! deve continuar responsivo. Os guests são compilados na primeira execução.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};
use superapp_core::{Decision, MemoryAudit};
use superapp_wasm_host::{
    DirStorage, Grants, Interface, Limits, MemoryStorage, MiniApp, Runtime, WasmError,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Compila os guests para wasm32-wasip2 (uma vez por processo) e devolve os bytes de um deles.
fn guest(dir: PathBuf, file: &str) -> Vec<u8> {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        for d in [
            root().join("crates/wasm-host/tests/guests/attack"),
            root().join("miniapps/counter"),
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

fn attack_wasm() -> Vec<u8> {
    guest(
        root().join("crates/wasm-host/tests/guests/attack"),
        "attack_guest.wasm",
    )
}

fn setup() -> (Runtime, Arc<MemoryAudit>) {
    let audit = Arc::new(MemoryAudit::default());
    (Runtime::new(audit.clone()).unwrap(), audit)
}

fn attack_app(rt: &Runtime) -> MiniApp {
    rt.load(
        "attack",
        &attack_wasm(),
        &Grants::all(),
        Arc::new(MemoryStorage::default()),
    )
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn echo_returns_output_logs_and_fuel() {
    let (rt, _) = setup();
    let out = attack_app(&rt)
        .run("echo:oi", &Limits::default())
        .await
        .unwrap();
    assert_eq!(out.output, "oi");
    assert_eq!(out.logs, vec!["echo"]);
    assert!(out.fuel_used > 0);
}

#[tokio::test(flavor = "current_thread")]
async fn app_error_is_reported_as_app_error() {
    let (rt, _) = setup();
    let e = attack_app(&rt)
        .run("fail:deu ruim", &Limits::default())
        .await
        .unwrap_err();
    assert_eq!(e, WasmError::App("deu ruim".into()));
}

#[tokio::test(flavor = "current_thread")]
async fn infinite_loop_is_stopped_by_fuel() {
    let (rt, _) = setup();
    let limits = Limits {
        fuel: 5_000_000,
        ..Limits::default()
    };
    let app = attack_app(&rt); // compilar o guest não conta no tempo medido
    let t = Instant::now();
    let e = app.run("spin", &limits).await.unwrap_err();
    assert_eq!(e, WasmError::OutOfFuel);
    assert!(t.elapsed() < Duration::from_secs(2));
}

#[tokio::test(flavor = "current_thread")]
async fn memory_hog_is_contained_and_reported_clearly() {
    let (rt, _) = setup();
    let app = attack_app(&rt);
    let limits = Limits {
        memory_bytes: 16 * 1024 * 1024,
        ..Limits::default()
    };
    let e = app.run("hog:64", &limits).await.unwrap_err();
    assert_eq!(
        e,
        WasmError::OutOfMemory {
            limit_bytes: 16 * 1024 * 1024
        }
    );
    // Dentro do limite funciona.
    assert_eq!(app.run("hog:4", &limits).await.unwrap().output, "4194304");
}

#[tokio::test(flavor = "current_thread")]
async fn fs_and_network_are_denied_without_permission() {
    let (rt, _) = setup();
    let app = attack_app(&rt);
    for cmd in ["fs", "net"] {
        let out = app.run(cmd, &Limits::default()).await.unwrap().output;
        assert!(out.starts_with("negado"), "{cmd}: {out}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn cpu_only_guest_hits_timeout_and_host_stays_responsive() {
    let (rt, _) = setup();
    let app = attack_app(&rt);
    let limits = Limits {
        fuel: u64::MAX / 2, // fuel "infinito": só o tempo pode parar
        timeout: Duration::from_millis(300),
        ..Limits::default()
    };
    // Runtime de uma thread só: se o guest monopolizasse a thread, o contador de ticks
    // abaixo ficaria em ~0.
    let ticker = async {
        let mut ticks = 0u32;
        let end = Instant::now() + Duration::from_millis(250);
        while Instant::now() < end {
            tokio::time::sleep(Duration::from_millis(10)).await;
            ticks += 1;
        }
        ticks
    };
    let (res, ticks) = tokio::join!(app.run("spin", &limits), ticker);
    assert_eq!(res.unwrap_err(), WasmError::Timeout);
    assert!(ticks >= 10, "host ficou sem responder (ticks = {ticks})");
}

#[tokio::test(flavor = "current_thread")]
async fn import_not_granted_fails_to_load_and_is_audited() {
    let (rt, audit) = setup();
    let grants = Grants::none().with(Interface::Log).with(Interface::Clock);
    let err = rt
        .load(
            "attack",
            &attack_wasm(),
            &grants,
            Arc::new(MemoryStorage::default()),
        )
        .err()
        .expect("deveria recusar");
    assert_eq!(
        err,
        WasmError::ImportNotGranted("superapp:host/storage".into())
    );
    let ev = audit.events();
    assert_eq!(ev[0].decision, Decision::DeniedNoCapability);
    assert_eq!(ev[0].window, "app:attack");
}

#[tokio::test(flavor = "current_thread")]
async fn garbage_and_bad_ids_are_rejected_at_load() {
    let (rt, _) = setup();
    let st = || Arc::new(MemoryStorage::default());
    assert!(matches!(
        rt.load("x", b"isto nao e wasm", &Grants::all(), st()),
        Err(WasmError::Load(_))
    ));
    for bad in ["", "../x", "A", "a/b", &"a".repeat(33)] {
        assert!(
            matches!(
                rt.load(bad, &attack_wasm(), &Grants::all(), st()),
                Err(WasmError::Load(_))
            ),
            "id {bad:?} deveria ser recusado"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn storage_roundtrip_is_isolated_between_apps_and_quota_applies() {
    let (rt, _) = setup();
    let a = attack_app(&rt);
    let b = attack_app(&rt); // outro storage
    let l = Limits::default();
    assert_eq!(a.run("set:k=valor", &l).await.unwrap().output, "ok");
    assert_eq!(a.run("get:k", &l).await.unwrap().output, "valor");
    assert_eq!(b.run("get:k", &l).await.unwrap().output, "<vazio>");
    // Valor acima de 1 MiB é recusado pelo host; o app recebe o erro, não derruba nada.
    let e = a.run("big", &l).await.unwrap_err();
    assert!(matches!(e, WasmError::App(m) if m.contains("valor acima")));
}

#[tokio::test(flavor = "current_thread")]
async fn dir_storage_blocks_path_traversal_and_bad_ids() {
    let tmp = std::env::temp_dir().join(format!("superapp-wasm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(DirStorage::open(&tmp, "../fora").is_err());
    assert!(DirStorage::open(&tmp, "A").is_err());
    let (rt, _) = setup();
    let st = Arc::new(DirStorage::open(&tmp, "attack").unwrap());
    let app = rt
        .load("attack", &attack_wasm(), &Grants::all(), st)
        .unwrap();
    let l = Limits::default();
    // Chave maliciosa vira nome hexadecimal dentro do diretório do app.
    assert_eq!(
        app.run("set:../../escapou=1", &l).await.unwrap().output,
        "ok"
    );
    assert!(!tmp.join("escapou").exists());
    assert!(!tmp.parent().unwrap().join("escapou").exists());
    assert_eq!(app.run("get:../../escapou", &l).await.unwrap().output, "1");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test(flavor = "current_thread")]
async fn every_host_call_is_audited_with_app_identity() {
    let (rt, audit) = setup();
    let app = attack_app(&rt);
    app.run("set:a=b", &Limits::default()).await.unwrap();
    app.run("now", &Limits::default()).await.unwrap();
    let cmds: Vec<_> = audit
        .events()
        .into_iter()
        .filter(|e| e.window == "app:attack")
        .map(|e| e.command)
        .collect();
    assert_eq!(cmds, vec!["storage.set", "clock.now_ms"]);
}

#[tokio::test(flavor = "current_thread")]
async fn example_counter_persists_between_runs_and_needs_only_log_and_storage() {
    let (rt, _) = setup();
    let wasm = guest(root().join("miniapps/counter"), "counter.wasm");
    let grants = Grants::none().with(Interface::Log).with(Interface::Storage);
    let app = rt
        .load(
            "counter",
            &wasm,
            &grants,
            Arc::new(MemoryStorage::default()),
        )
        .unwrap();
    let l = Limits::default();
    assert_eq!(app.run("", &l).await.unwrap().output, "execuções: 1");
    let out = app.run("", &l).await.unwrap();
    assert_eq!(out.output, "execuções: 2");
    assert_eq!(out.logs, vec!["execução nº 2"]);
    // Sem `storage` concedido, não carrega.
    let err = rt
        .load(
            "counter",
            &wasm,
            &Grants::none().with(Interface::Log),
            Arc::new(MemoryStorage::default()),
        )
        .err()
        .unwrap();
    assert!(matches!(err, WasmError::ImportNotGranted(_)));
}

// ---- E2.3: pacote assinado de ponta a ponta -------------------------------------------

mod packaged {
    use super::*;
    use ed25519_dalek::SigningKey;
    use superapp_package::{
        pack, verify, Manifest, PackageError, Permission, RequestedLimits, TrustStore,
    };

    fn author() -> SigningKey {
        SigningKey::from_bytes(&[42; 32])
    }

    fn trust() -> TrustStore {
        TrustStore::new(vec![author().verifying_key().to_bytes()])
    }

    fn manifest(id: &str, perms: Vec<Permission>) -> Manifest {
        Manifest {
            schema: 1,
            id: id.into(),
            name: "Counter".into(),
            version: "1.0.0".into(),
            permissions: perms,
            wasm_sha256: String::new(),
            limits: None,
        }
    }

    fn counter_wasm() -> Vec<u8> {
        guest(root().join("miniapps/counter"), "counter.wasm")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn signed_package_verifies_loads_and_runs() {
        let (rt, _) = setup();
        let m = manifest("counter", vec![Permission::Log, Permission::Storage]);
        let bytes = pack(&author(), &m, &counter_wasm()).unwrap();
        let pkg = verify(&bytes, &trust()).unwrap();
        let approved = Grants::from_permissions(&pkg.manifest.permissions);
        let app = rt
            .load_verified(&pkg, &approved, Arc::new(MemoryStorage::default()))
            .unwrap();
        let out = app
            .run("", &Limits::from_manifest(&pkg.manifest))
            .await
            .unwrap();
        assert_eq!(out.output, "execuções: 1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn code_importing_more_than_the_manifest_declares_is_refused() {
        // O mini-app usa `storage`, mas o manifesto só declara `log`: mesmo assinado e com
        // tudo aprovado, não carrega (o usuário nunca viu a permissão de storage).
        let (rt, _) = setup();
        let m = manifest("counter", vec![Permission::Log]);
        let pkg = verify(&pack(&author(), &m, &counter_wasm()).unwrap(), &trust()).unwrap();
        let approved = Grants::from_permissions(&pkg.manifest.permissions);
        let err = rt
            .load_verified(&pkg, &approved, Arc::new(MemoryStorage::default()))
            .err()
            .unwrap();
        assert!(matches!(err, WasmError::ImportNotGranted(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn approval_cannot_exceed_what_the_manifest_requested() {
        let (rt, _) = setup();
        let m = manifest("counter", vec![Permission::Log, Permission::Storage]);
        let pkg = verify(&pack(&author(), &m, &counter_wasm()).unwrap(), &trust()).unwrap();
        let err = rt
            .load_verified(&pkg, &Grants::all(), Arc::new(MemoryStorage::default()))
            .err()
            .unwrap();
        assert!(matches!(err, WasmError::Load(m) if m.contains("maior")));
    }

    #[test]
    fn tampered_real_package_never_reaches_the_runtime() {
        let m = manifest("counter", vec![Permission::Log, Permission::Storage]);
        let mut bytes = pack(&author(), &m, &counter_wasm()).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        assert_eq!(
            verify(&bytes, &trust()).unwrap_err(),
            PackageError::HashMismatch
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn manifest_limits_are_applied_to_execution() {
        let (rt, _) = setup();
        let mut m = manifest(
            "attack",
            vec![Permission::Log, Permission::Storage, Permission::Clock],
        );
        m.limits = Some(RequestedLimits {
            fuel: Some(2_000_000),
            ..Default::default()
        });
        let pkg = verify(&pack(&author(), &m, &attack_wasm()).unwrap(), &trust()).unwrap();
        let approved = Grants::from_permissions(&pkg.manifest.permissions);
        let app = rt
            .load_verified(&pkg, &approved, Arc::new(MemoryStorage::default()))
            .unwrap();
        let e = app
            .run("spin", &Limits::from_manifest(&pkg.manifest))
            .await
            .unwrap_err();
        assert_eq!(e, WasmError::OutOfFuel);
    }
}
