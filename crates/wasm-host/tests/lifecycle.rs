//! Ciclo de vida de mini-apps (E2.4): instalar, consentir, atualizar, suspender, remover.
mod common;

use common::{clicker_wasm, tarefas_wasm};
use ed25519_dalek::SigningKey;
use std::path::PathBuf;
use std::sync::Arc;
use superapp_core::MemoryAudit;
use superapp_package::{pack, Manifest, Permission, TrustStore};
use superapp_wasm_host::lifecycle::{LifecycleError, Registry};
use superapp_wasm_host::ui::{Event, Node};
use superapp_wasm_host::WasmError;

fn author() -> SigningKey {
    SigningKey::from_bytes(&[11; 32])
}

fn trust() -> TrustStore {
    TrustStore::new(vec![author().verifying_key().to_bytes()])
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("superapp-lc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn package(version: &str, perms: Vec<Permission>) -> Vec<u8> {
    let m = Manifest {
        schema: 1,
        id: "clicker".into(),
        name: "Clicker".into(),
        version: version.into(),
        permissions: perms,
        wasm_sha256: String::new(),
        limits: None,
    };
    pack(&author(), &m, &clicker_wasm()).unwrap()
}

fn open(root: &PathBuf, trust: TrustStore) -> Registry {
    Registry::open(root, trust, Arc::new(MemoryAudit::default())).unwrap()
}

fn click() -> Event {
    Event::click("inc")
}

fn text_of(n: &Node) -> String {
    match n {
        Node::Stack { children } => match &children[0] {
            Node::Text { text } => text.clone(),
            other => panic!("esperava texto, veio {other:?}"),
        },
        other => panic!("esperava stack, veio {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn install_with_consent_then_interact_and_persist() {
    let root = tmp("flow");
    let reg = open(&root, trust());
    let pkg = package("1.0.0", vec![Permission::Storage]);

    let pv = reg.inspect(&pkg).unwrap();
    assert_eq!(pv.id, "clicker");
    assert!(!pv.is_update);
    assert_eq!(pv.requested, vec![Permission::Storage]);
    assert_eq!(pv.added, vec![Permission::Storage]);
    assert!(reg.list().is_empty(), "inspecionar não instala");

    let inst = reg.install(&pkg, &[Permission::Storage]).unwrap();
    assert!(inst.enabled && inst.problem.is_none());
    assert_eq!(
        text_of(&reg.dispatch("clicker", &Event::Init).await.unwrap()),
        "Cliques: 0"
    );
    reg.dispatch("clicker", &click()).await.unwrap();
    let ui = reg.dispatch("clicker", &click()).await.unwrap();
    assert_eq!(text_of(&ui), "Cliques: 2");

    // Reabrir o registro (reinício do app): estado e dados continuam.
    drop(reg);
    let reg = open(&root, trust());
    assert_eq!(reg.list().len(), 1);
    assert_eq!(
        text_of(&reg.dispatch("clicker", &Event::Init).await.unwrap()),
        "Cliques: 2"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn denying_a_permission_keeps_the_app_from_running() {
    let root = tmp("deny");
    let reg = open(&root, trust());
    // O usuário instala sem aprovar `storage`: o app (que precisa dele) não carrega.
    reg.install(&package("1.0.0", vec![Permission::Storage]), &[])
        .unwrap();
    let e = reg.dispatch("clicker", &Event::Init).await.unwrap_err();
    assert!(
        matches!(e, LifecycleError::Wasm(WasmError::ImportNotGranted(_))),
        "{e:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn approval_beyond_the_manifest_is_refused() {
    let root = tmp("exceed");
    let reg = open(&root, trust());
    let e = reg
        .install(
            &package("1.0.0", vec![Permission::Storage]),
            &[Permission::Storage, Permission::Clock],
        )
        .unwrap_err();
    assert_eq!(e, LifecycleError::ApprovalExceedsRequest);
    assert!(reg.list().is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn updates_must_increase_version_and_report_added_permissions_and_keep_data() {
    let root = tmp("update");
    let reg = open(&root, trust());
    reg.install(
        &package("1.0.0", vec![Permission::Storage]),
        &[Permission::Storage],
    )
    .unwrap();
    reg.dispatch("clicker", &click()).await.unwrap();

    // Mesma versão e versão menor: anti-rollback.
    for v in ["1.0.0", "0.9.9"] {
        let e = reg
            .install(
                &package(v, vec![Permission::Storage]),
                &[Permission::Storage],
            )
            .unwrap_err();
        assert!(matches!(e, LifecycleError::Rollback { .. }), "{v}: {e:?}");
    }

    // Versão maior pedindo uma permissão nova: o preview mostra o que mudou.
    let newer = package("1.1.0", vec![Permission::Storage, Permission::Clock]);
    let pv = reg.inspect(&newer).unwrap();
    assert!(pv.is_update);
    assert_eq!(pv.installed_version.as_deref(), Some("1.0.0"));
    assert_eq!(pv.previously_approved, vec![Permission::Storage]);
    assert_eq!(pv.added, vec![Permission::Clock]);

    reg.install(&newer, &[Permission::Storage]).unwrap(); // usuário não aprova `clock`
    let info = &reg.list()[0];
    assert_eq!(info.version, "1.1.0");
    assert_eq!(info.approved, vec![Permission::Storage]);
    // Os dados do app sobrevivem à atualização.
    assert_eq!(
        text_of(&reg.dispatch("clicker", &Event::Init).await.unwrap()),
        "Cliques: 1"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn suspend_and_resume_survive_restart() {
    let root = tmp("suspend");
    let reg = open(&root, trust());
    reg.install(
        &package("1.0.0", vec![Permission::Storage]),
        &[Permission::Storage],
    )
    .unwrap();
    reg.set_enabled("clicker", false).unwrap();
    assert_eq!(
        reg.dispatch("clicker", &Event::Init).await.unwrap_err(),
        LifecycleError::Disabled
    );
    drop(reg);
    let reg = open(&root, trust());
    assert!(!reg.list()[0].enabled, "suspensão persiste");
    reg.set_enabled("clicker", true).unwrap();
    assert!(reg.dispatch("clicker", &Event::Init).await.is_ok());
    assert_eq!(
        reg.set_enabled("nao-existe", true).unwrap_err(),
        LifecycleError::NotInstalled
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn uninstall_removes_app_and_data_unless_kept() {
    let root = tmp("uninstall");
    let reg = open(&root, trust());
    let pkg = package("1.0.0", vec![Permission::Storage]);
    reg.install(&pkg, &[Permission::Storage]).unwrap();
    reg.dispatch("clicker", &click()).await.unwrap();

    reg.uninstall("clicker", true).unwrap(); // mantém os dados
    assert!(reg.list().is_empty());
    assert_eq!(
        reg.dispatch("clicker", &Event::Init).await.unwrap_err(),
        LifecycleError::NotInstalled
    );
    reg.install(&pkg, &[Permission::Storage]).unwrap();
    assert_eq!(
        text_of(&reg.dispatch("clicker", &Event::Init).await.unwrap()),
        "Cliques: 1"
    );

    reg.uninstall("clicker", false).unwrap(); // apaga tudo
    reg.install(&pkg, &[Permission::Storage]).unwrap();
    assert_eq!(
        text_of(&reg.dispatch("clicker", &Event::Init).await.unwrap()),
        "Cliques: 0"
    );

    // Ids maliciosos não saem da raiz.
    assert_eq!(
        reg.uninstall("../apps", false).unwrap_err(),
        LifecycleError::NotInstalled
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn untrusted_or_tampered_packages_never_install_and_revocation_stops_running_apps() {
    let root = tmp("trust");
    let reg = open(&root, trust());
    let pkg = package("1.0.0", vec![Permission::Storage]);

    // Autor desconhecido.
    let other = open(&tmp("trust-other"), TrustStore::new(vec![[9; 32]]));
    assert!(matches!(
        other.inspect(&pkg),
        Err(LifecycleError::Package(_))
    ));
    // Pacote adulterado.
    let mut bad = pkg.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert!(matches!(
        reg.install(&bad, &[]),
        Err(LifecycleError::Package(_))
    ));
    assert!(reg.list().is_empty());

    reg.install(&pkg, &[Permission::Storage]).unwrap();
    drop(reg);

    // Chave do autor removida da lista (revogação): o app instalado passa a "com problema".
    let revoked = open(&root, TrustStore::default());
    let info = &revoked.list()[0];
    assert!(info.problem.is_some() && !info.enabled);
    assert!(matches!(
        revoked.dispatch("clicker", &Event::Init).await,
        Err(LifecycleError::Broken(_))
    ));
    assert!(matches!(
        revoked.set_enabled("clicker", true),
        Err(LifecycleError::Broken(_))
    ));

    // Pacote em disco adulterado depois de instalado também é detectado ao reabrir.
    let path = root.join("apps/clicker/package.sapp");
    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    let tampered = open(&root, trust());
    assert!(tampered.list()[0].problem.is_some());
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn events_with_bad_ids_are_rejected_before_reaching_the_app() {
    let root = tmp("event");
    let reg = open(&root, trust());
    reg.install(
        &package("1.0.0", vec![Permission::Storage]),
        &[Permission::Storage],
    )
    .unwrap();
    for bad in ["../x", "", "A b", &"a".repeat(33)] {
        let e = reg
            .dispatch("clicker", &Event::click(bad))
            .await
            .unwrap_err();
        assert!(matches!(e, LifecycleError::InvalidUi(_)), "{bad:?}: {e:?}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn data_can_be_exported_and_restored_across_reinstall() {
    let root = tmp("backup");
    let reg = open(&root, trust());
    let pkg = package("1.0.0", vec![Permission::Storage]);
    reg.install(&pkg, &[Permission::Storage]).unwrap();
    reg.dispatch("clicker", &click()).await.unwrap();
    reg.dispatch("clicker", &click()).await.unwrap();
    let backup = reg.export_data("clicker").unwrap();

    // Remove apagando os dados, reinstala: contador zerado; restaura: volta a 2.
    reg.uninstall("clicker", false).unwrap();
    reg.install(&pkg, &[Permission::Storage]).unwrap();
    assert_eq!(
        text_of(&reg.dispatch("clicker", &Event::Init).await.unwrap()),
        "Cliques: 0"
    );
    assert_eq!(reg.import_data("clicker", &backup).unwrap(), 1);
    assert_eq!(
        text_of(&reg.dispatch("clicker", &Event::Init).await.unwrap()),
        "Cliques: 2"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn backup_of_an_unknown_app_or_for_another_app_is_refused() {
    let root = tmp("backup-refused");
    let reg = open(&root, trust());
    assert_eq!(
        reg.export_data("clicker").unwrap_err(),
        LifecycleError::NotInstalled
    );
    assert_eq!(
        reg.import_data("clicker", b"{}").unwrap_err(),
        LifecycleError::NotInstalled
    );
    reg.install(
        &package("1.0.0", vec![Permission::Storage]),
        &[Permission::Storage],
    )
    .unwrap();
    let other = br#"{"format":"superapp-backup-1","app_id":"outro","app_version":"1","created_at":1,"entries":[]}"#;
    assert!(matches!(
        reg.import_data("clicker", other),
        Err(LifecycleError::Io(_))
    ));
}

// ---- Tarefas: campos de entrada, lista com ações e progresso, de ponta a ponta no Wasm ----

fn tarefas_pkg() -> Vec<u8> {
    let m = Manifest {
        schema: 1,
        id: "tarefas".into(),
        name: "Tarefas".into(),
        version: "0.1.0".into(),
        permissions: vec![Permission::Storage],
        wasm_sha256: String::new(),
        limits: None,
    };
    pack(&author(), &m, &tarefas_wasm()).unwrap()
}

fn add(texto: &str) -> Event {
    let mut e = Event::click("add");
    if let Event::Click { values, .. } = &mut e {
        values.insert("nova".into(), texto.into());
    }
    e
}

/// Itens da lista de ações e o rótulo do progresso, para conferir a tela inteira.
type Itens = Vec<(String, Option<bool>)>;

fn resumo(n: &Node) -> (Itens, Option<String>, Vec<String>) {
    fn walk(n: &Node, itens: &mut Itens, prog: &mut Option<String>, textos: &mut Vec<String>) {
        match n {
            Node::Stack { children } | Node::Row { children } => {
                children.iter().for_each(|c| walk(c, itens, prog, textos))
            }
            Node::ActionList { items } => {
                itens.extend(items.iter().map(|i| (i.text.clone(), i.done)))
            }
            Node::Progress { label, .. } => *prog = Some(label.clone()),
            Node::Text { text } => textos.push(text.clone()),
            _ => {}
        }
    }
    let (mut itens, mut prog, mut textos) = (Vec::new(), None, Vec::new());
    walk(n, &mut itens, &mut prog, &mut textos);
    (itens, prog, textos)
}

#[tokio::test(flavor = "current_thread")]
async fn tarefas_flow_with_inputs_actions_and_progress() {
    let root = tmp("tarefas");
    let reg = open(&root, trust());
    reg.install(&tarefas_pkg(), &[Permission::Storage]).unwrap();

    // Vazio: só a mensagem.
    let ui = reg.dispatch("tarefas", &Event::Init).await.unwrap();
    let (itens, prog, textos) = resumo(&ui);
    assert!(itens.is_empty() && prog.is_none());
    assert_eq!(textos, vec!["Nenhuma tarefa ainda."]);

    // Adicionar (com quebra de linha e espaços: vira uma linha só). Campo vazio avisa.
    reg.dispatch("tarefas", &add("  comprar\npão "))
        .await
        .unwrap();
    reg.dispatch("tarefas", &add("ligar para o banco"))
        .await
        .unwrap();
    let (itens, _, _) = resumo(&reg.dispatch("tarefas", &add("   ")).await.unwrap());
    assert_eq!(
        itens,
        vec![
            ("comprar pão".to_string(), Some(false)),
            ("ligar para o banco".to_string(), Some(false)),
        ]
    );
    let (_, _, textos) = resumo(&reg.dispatch("tarefas", &add("")).await.unwrap());
    assert!(textos.iter().any(|t| t.contains("Digite a tarefa")));

    // Marcar a primeira como feita: progresso 1 de 2, e o estado persiste entre execuções.
    let ui = reg
        .dispatch("tarefas", &Event::click("feito-0"))
        .await
        .unwrap();
    let (itens, prog, _) = resumo(&ui);
    assert_eq!(itens[0], ("comprar pão".to_string(), Some(true)));
    assert_eq!(prog.as_deref(), Some("1 de 2 concluídas"));
    let (itens, _, _) = resumo(&reg.dispatch("tarefas", &Event::Init).await.unwrap());
    assert_eq!(itens[0].1, Some(true));

    // Desfazer, remover a segunda, e ids fora do intervalo não quebram nada.
    reg.dispatch("tarefas", &Event::click("feito-0"))
        .await
        .unwrap();
    reg.dispatch("tarefas", &Event::click("feito-99"))
        .await
        .unwrap();
    reg.dispatch("tarefas", &Event::click("rm-99"))
        .await
        .unwrap();
    let (itens, _, _) = resumo(
        &reg.dispatch("tarefas", &Event::click("rm-1"))
            .await
            .unwrap(),
    );
    assert_eq!(itens, vec![("comprar pão".to_string(), Some(false))]);

    // Limpar concluídas.
    reg.dispatch("tarefas", &Event::click("feito-0"))
        .await
        .unwrap();
    let (itens, _, textos) = resumo(
        &reg.dispatch("tarefas", &Event::click("limpar"))
            .await
            .unwrap(),
    );
    assert!(itens.is_empty());
    assert_eq!(textos, vec!["Nenhuma tarefa ainda."]);
}

#[tokio::test(flavor = "current_thread")]
async fn tarefas_respects_its_own_limits() {
    let root = tmp("tarefas-limits");
    let reg = open(&root, trust());
    reg.install(&tarefas_pkg(), &[Permission::Storage]).unwrap();
    for i in 0..50 {
        reg.dispatch("tarefas", &add(&format!("t{i}")))
            .await
            .unwrap();
    }
    // A 51ª é recusada com aviso, e a tela cheia ainda passa pela validação da UI.
    let ui = reg.dispatch("tarefas", &add("a mais")).await.unwrap();
    let (itens, _, textos) = resumo(&ui);
    assert_eq!(itens.len(), 50);
    assert!(textos.iter().any(|t| t.contains("Limite de 50")));
    // Texto enorme é cortado em 200 caracteres (e a tela continua válida).
    let reg = open(&tmp("tarefas-long"), trust());
    reg.install(&tarefas_pkg(), &[Permission::Storage]).unwrap();
    let ui = reg
        .dispatch("tarefas", &add(&"x".repeat(900)))
        .await
        .unwrap();
    let (itens, _, _) = resumo(&ui);
    assert_eq!(itens[0].0.chars().count(), 200);
}
