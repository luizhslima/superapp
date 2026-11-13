//! Rede mediada de ponta a ponta (ADR-011): guest Wasm -> interface `http` -> política -> backend
//! falso. Nada aqui toca a internet.
mod common;

use common::net_wasm;
use ed25519_dalek::SigningKey;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use superapp_core::{Decision, MemoryAudit};
use superapp_package::{pack, Manifest, Network, Permission, TrustStore};
use superapp_wasm_host::lifecycle::{LifecycleError, Registry};
use superapp_wasm_host::net::{BoxFuture, NetBackend, Pinned, RawResponse, SendError};
use superapp_wasm_host::ui::{Event, Node};
use superapp_wasm_host::WasmError;

const ID: &str = "netcheck";

fn author() -> SigningKey {
    SigningKey::from_bytes(&[13; 32])
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("superapp-net-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Backend falso: DNS configurável, resposta fixa e registro do que "saiu".
#[derive(Default)]
struct Mock {
    dns: Vec<(String, IpAddr)>,
    sent: Mutex<Vec<String>>,
}

impl NetBackend for Mock {
    fn resolve<'a>(&'a self, host: &'a str) -> BoxFuture<'a, Result<Vec<IpAddr>, String>> {
        Box::pin(async move {
            Ok(vec![self
                .dns
                .iter()
                .find(|(h, _)| h == host)
                .map(|(_, ip)| *ip)
                .unwrap_or_else(|| "93.184.216.34".parse().unwrap())])
        })
    }
    fn send<'a>(&'a self, r: Pinned<'a>) -> BoxFuture<'a, Result<RawResponse, SendError>> {
        Box::pin(async move {
            self.sent.lock().unwrap().push(format!(
                "{:?} {} body={}",
                r.method,
                r.url,
                r.body.len()
            ));
            Ok(RawResponse {
                status: 200,
                content_type: "text/plain; charset=utf-8".into(),
                location: None,
                body: "olá, mundo".as_bytes().to_vec(),
            })
        })
    }
}

struct Setup {
    reg: Registry,
    audit: Arc<MemoryAudit>,
    mock: Arc<Mock>,
}

fn manifest(version: &str, perms: Vec<Permission>, hosts: Option<&[&str]>) -> Manifest {
    Manifest {
        schema: 1,
        id: ID.into(),
        name: "Netcheck".into(),
        version: version.into(),
        permissions: perms,
        wasm_sha256: String::new(),
        limits: None,
        network: hosts.map(|h| Network {
            hosts: h.iter().map(|x| x.to_string()).collect(),
        }),
    }
}

fn setup(name: &str, mock: Mock, with_backend: bool) -> Setup {
    let trust = TrustStore::new(vec![author().verifying_key().to_bytes()]);
    let audit = Arc::new(MemoryAudit::default());
    let mut reg = Registry::open(tmp(name), trust, audit.clone()).unwrap();
    let mock = Arc::new(mock);
    if with_backend {
        reg.set_net_backend(mock.clone());
    }
    Setup { reg, audit, mock }
}

fn pkg(m: &Manifest) -> Vec<u8> {
    pack(&author(), m, &net_wasm()).unwrap()
}

fn clique(id: &str, url: &str) -> Event {
    let mut e = Event::click(id);
    if let Event::Click { values, .. } = &mut e {
        values.insert("url".into(), url.into());
    }
    e
}

fn linhas(n: &Node) -> Vec<String> {
    match n {
        Node::Stack { children } => children
            .iter()
            .filter_map(|c| match c {
                Node::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect(),
        other => panic!("esperava stack, veio {other:?}"),
    }
}

async fn rodar(s: &Setup, id: &str, url: &str) -> Vec<String> {
    linhas(&s.reg.dispatch(ID, &clique(id, url)).await.unwrap())
}

fn instalar_com_http(s: &Setup, hosts: &[&str]) {
    let m = manifest("0.1.0", vec![Permission::Http], Some(hosts));
    s.reg.install(&pkg(&m), &[Permission::Http]).unwrap();
}

fn negacoes(s: &Setup) -> Vec<String> {
    s.audit
        .events()
        .into_iter()
        .filter(|e| e.decision == Decision::DeniedByNetworkRule)
        .map(|e| e.command)
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn allowed_get_and_post_work_and_are_audited_without_the_query() {
    let s = setup("ok", Mock::default(), true);
    instalar_com_http(&s, &["api.exemplo.com", "*.reddit.com"]);

    let out = rodar(&s, "get", "https://api.exemplo.com/v1/x?segredo=1").await;
    assert_eq!(
        out,
        vec!["ok status=200 bytes=11 tipo=text/plain; charset=utf-8 corpo=olá, mundo"]
    );
    let out = rodar(&s, "post", "https://old.reddit.com/api").await;
    assert!(out[0].starts_with("ok status=200"), "{out:?}");
    assert_eq!(
        s.mock.sent.lock().unwrap().as_slice(),
        [
            "Get https://api.exemplo.com/v1/x?segredo=1 body=0",
            "Post https://old.reddit.com/api body=7"
        ]
    );
    // Auditoria: host, método, status e bytes; a query (que pode ter segredo) não entra.
    let cmds: Vec<String> = s.audit.events().into_iter().map(|e| e.command).collect();
    assert!(cmds
        .iter()
        .any(|c| c == "http.fetch GET api.exemplo.com -> 200 sent=0 recv=11"));
    assert!(cmds
        .iter()
        .any(|c| c == "http.fetch POST old.reddit.com -> 200 sent=7 recv=11"));
    assert!(cmds.iter().all(|c| !c.contains("segredo")));
    assert!(negacoes(&s).is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn rule_violations_are_refused_audited_and_never_reach_the_backend() {
    let mock = Mock {
        dns: vec![("interno.exemplo.com".into(), "10.0.0.5".parse().unwrap())],
        ..Default::default()
    };
    let s = setup("deny", mock, true);
    instalar_com_http(&s, &["api.exemplo.com", "interno.exemplo.com"]);

    for (url, esperado) in [
        ("https://evil.com/", "domínio fora da lista aprovada"),
        (
            "https://api.exemplo.com.evil.com/",
            "domínio fora da lista aprovada",
        ),
        ("http://api.exemplo.com/", "URL inválida: só https"),
        (
            "https://127.0.0.1/",
            "URL inválida: IP literal não é permitido",
        ),
        (
            "https://api.exemplo.com:8443/",
            "URL inválida: só a porta 443",
        ),
        (
            "https://u:p@api.exemplo.com/",
            "URL inválida: sem usuário ou senha na URL",
        ),
        (
            "https://interno.exemplo.com/",
            "endereço de rede não público",
        ),
    ] {
        let out = rodar(&s, "get", url).await;
        assert_eq!(out, vec![format!("erro {esperado}")], "{url}");
    }
    assert!(s.mock.sent.lock().unwrap().is_empty(), "nada saiu");
    assert_eq!(negacoes(&s).len(), 7);
    assert!(negacoes(&s)[0].starts_with("http.fetch GET evil.com -> negado"));

    // Cabeçalho não permitido (Authorization) também é recusado.
    let out = rodar(&s, "cabecalho", "https://api.exemplo.com/").await;
    assert_eq!(out, vec!["erro cabeçalho não permitido"]);
    assert!(s.mock.sent.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn at_most_four_calls_per_run_and_the_counter_resets_each_run() {
    let s = setup("calls", Mock::default(), true);
    instalar_com_http(&s, &["api.exemplo.com"]);
    let out = rodar(&s, "muitas", "https://api.exemplo.com/").await;
    assert_eq!(out.len(), 6);
    assert!(out[..4].iter().all(|l| l.starts_with("ok ")), "{out:?}");
    assert!(out[4..]
        .iter()
        .all(|l| l == "erro limite de 4 chamadas por execução"));
    assert_eq!(s.mock.sent.lock().unwrap().len(), 4);
    assert_eq!(negacoes(&s).len(), 2);
    // Outra execução começa do zero (até o teto por minuto: 30).
    let out = rodar(&s, "get", "https://api.exemplo.com/").await;
    assert!(out[0].starts_with("ok "));
}

#[tokio::test(flavor = "current_thread")]
async fn rate_limit_per_minute_caps_an_app_across_runs() {
    let s = setup("rate", Mock::default(), true);
    instalar_com_http(&s, &["api.exemplo.com"]);
    let mut ok = 0;
    let mut barradas = 0;
    for _ in 0..8 {
        for l in rodar(&s, "muitas", "https://api.exemplo.com/").await {
            if l.starts_with("ok ") {
                ok += 1;
            } else if l.contains("por minuto") {
                barradas += 1;
            }
        }
    }
    assert_eq!(ok, 30, "teto de 30 por minuto");
    assert!(barradas > 0);
    assert_eq!(s.mock.sent.lock().unwrap().len(), 30);
}

#[tokio::test(flavor = "current_thread")]
async fn without_the_grant_or_the_manifest_the_import_fails_to_load() {
    // Declarou `http`, mas o usuário não aprovou nada.
    let s = setup("nogrant", Mock::default(), true);
    let m = manifest("0.1.0", vec![Permission::Http], Some(&["api.exemplo.com"]));
    s.reg.install(&pkg(&m), &[]).unwrap();
    let e = s
        .reg
        .dispatch(ID, &clique("get", "https://api.exemplo.com/"))
        .await;
    assert!(
        matches!(
            e,
            Err(LifecycleError::Wasm(WasmError::ImportNotGranted(ref i))) if i == "superapp:host/http"
        ),
        "{e:?}"
    );
    assert!(s
        .audit
        .events()
        .iter()
        .any(|e| e.decision == Decision::DeniedNoCapability && e.command.contains("host/http")));

    // O código importa `http`, mas o manifesto nem pediu.
    let s = setup("nomanifest", Mock::default(), true);
    let m = manifest("0.1.0", vec![], None);
    s.reg.install(&pkg(&m), &[]).unwrap();
    let e = s
        .reg
        .dispatch(ID, &clique("get", "https://api.exemplo.com/"))
        .await;
    assert!(
        matches!(e, Err(LifecycleError::Wasm(WasmError::ImportNotGranted(_)))),
        "{e:?}"
    );
    assert!(s.mock.sent.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn host_without_a_network_backend_fails_clearly() {
    let s = setup("nobackend", Mock::default(), false);
    instalar_com_http(&s, &["api.exemplo.com"]);
    let out = rodar(&s, "get", "https://api.exemplo.com/").await;
    assert_eq!(out, vec!["erro rede indisponível neste host"]);
}

#[tokio::test(flavor = "current_thread")]
async fn consent_shows_the_domains_and_a_wider_update_asks_again() {
    let s = setup("consent", Mock::default(), true);
    let v1 = manifest("0.1.0", vec![Permission::Http], Some(&["api.exemplo.com"]));
    let p = s.reg.inspect(&pkg(&v1)).unwrap();
    assert_eq!(p.hosts, vec!["api.exemplo.com"]);
    assert_eq!(p.added, vec![Permission::Http]);
    s.reg.install(&pkg(&v1), &[Permission::Http]).unwrap();

    // Mesma lista de domínios: nada novo a aprovar.
    let v2 = manifest("0.2.0", vec![Permission::Http], Some(&["api.exemplo.com"]));
    assert!(s.reg.inspect(&pkg(&v2)).unwrap().added.is_empty());
    // Domínio a mais: `http` volta como permissão nova, mesmo já aprovada.
    let v3 = manifest(
        "0.3.0",
        vec![Permission::Http],
        Some(&["api.exemplo.com", "outro.exemplo.com"]),
    );
    let p = s.reg.inspect(&pkg(&v3)).unwrap();
    assert!(p.is_update);
    assert_eq!(p.added, vec![Permission::Http]);
    assert_eq!(p.hosts.len(), 2);
    // Um curinga conta como domínio novo (é mais amplo que o domínio exato).
    let v4 = manifest("0.4.0", vec![Permission::Http], Some(&["*.exemplo.com"]));
    assert_eq!(
        s.reg.inspect(&pkg(&v4)).unwrap().added,
        vec![Permission::Http]
    );
}
