//! Mini-app `discovery` (bancada manual de Discovery), de ponta a ponta no Wasm.
mod common;

use common::discovery_wasm;
use ed25519_dalek::SigningKey;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use superapp_core::MemoryAudit;
use superapp_package::{pack, Manifest, Permission, TrustStore};
use superapp_wasm_host::lifecycle::Registry;
use superapp_wasm_host::ui::{Event, Node};

const ID: &str = "discovery";

fn author() -> SigningKey {
    SigningKey::from_bytes(&[12; 32])
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("superapp-disc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn registry(name: &str) -> Registry {
    let trust = TrustStore::new(vec![author().verifying_key().to_bytes()]);
    let reg = Registry::open(tmp(name), trust, Arc::new(MemoryAudit::default())).unwrap();
    let m = Manifest {
        schema: 1,
        id: ID.into(),
        name: "Discovery".into(),
        version: "0.1.0".into(),
        permissions: vec![Permission::Storage, Permission::Clock],
        wasm_sha256: String::new(),
        limits: None,
        network: None,
    };
    let pkg = pack(&author(), &m, &discovery_wasm()).unwrap();
    reg.install(&pkg, &[Permission::Storage, Permission::Clock])
        .unwrap();
    reg
}

/// O que há na tela, achatado, para conferir sem depender do layout.
#[derive(Default, Debug)]
struct Vista {
    textos: Vec<String>,
    botoes: Vec<String>,
    itens: Vec<String>,
    /// itens de `list` (painel, roteiro)
    listas: Vec<String>,
    /// `input` e `textarea`: id -> valor
    campos: BTreeMap<String, String>,
    /// id -> (opções, escolhida)
    selects: BTreeMap<String, (Vec<String>, String)>,
    checks: BTreeMap<String, bool>,
}

fn vista(n: &Node) -> Vista {
    fn walk(n: &Node, v: &mut Vista) {
        match n {
            Node::Stack { children } | Node::Row { children } => {
                children.iter().for_each(|c| walk(c, v))
            }
            Node::Text { text } => v.textos.push(text.clone()),
            Node::List { items } => v.listas.extend(items.iter().cloned()),
            Node::Button { id, .. } => v.botoes.push(id.clone()),
            Node::ActionList { items } => v.itens.extend(items.iter().map(|i| i.text.clone())),
            Node::Input { id, value, .. } | Node::Textarea { id, value, .. } => {
                v.campos.insert(id.clone(), value.clone());
            }
            Node::Select {
                id,
                options,
                selected,
                ..
            } => {
                v.selects
                    .insert(id.clone(), (options.clone(), selected.clone()));
            }
            Node::Checkbox { id, checked, .. } => {
                v.checks.insert(id.clone(), *checked);
            }
            _ => {}
        }
    }
    let mut v = Vista::default();
    walk(n, &mut v);
    v
}

fn ev(id: &str, values: &[(&str, &str)]) -> Event {
    let mut e = Event::click(id);
    if let Event::Click { values: m, .. } = &mut e {
        for (k, val) in values {
            m.insert((*k).into(), (*val).into());
        }
    }
    e
}

async fn passo(reg: &Registry, id: &str, values: &[(&str, &str)]) -> Vista {
    vista(&reg.dispatch(ID, &ev(id, values)).await.unwrap())
}

async fn novo_territorio(reg: &Registry, nome: &str) -> Vista {
    passo(reg, "nav-t", &[]).await;
    passo(reg, "novo", &[]).await;
    passo(
        reg,
        "salvar",
        &[("f-nome", nome), ("f-processo", "fechamento")],
    )
    .await
}

#[tokio::test(flavor = "current_thread")]
async fn full_flow_territory_hypothesis_interview_and_removals() {
    let reg = registry("flow");

    // Vazio na abertura.
    let v = vista(&reg.dispatch(ID, &Event::Init).await.unwrap());
    assert!(v.itens.is_empty());
    assert!(v.textos.iter().any(|t| t.contains("Nenhum território")));

    // Hipótese sem território é recusada com explicação.
    passo(&reg, "nav-h", &[]).await;
    let v = passo(&reg, "novo", &[]).await;
    assert!(v.textos.iter().any(|t| t.contains("Crie um território")));
    passo(&reg, "cancelar", &[]).await;

    // Território: nome obrigatório (a ficha continua aberta com o que foi digitado).
    passo(&reg, "nav-t", &[]).await;
    passo(&reg, "novo", &[]).await;
    let v = passo(
        &reg,
        "salvar",
        &[("f-nome", "  "), ("f-processo", "conciliação")],
    )
    .await;
    assert!(v.textos.iter().any(|t| t.contains("nome ao território")));
    assert_eq!(v.campos["f-processo"], "conciliação");
    let v = passo(
        &reg,
        "salvar",
        &[("f-nome", "Financeiro"), ("f-processo", "conciliação")],
    )
    .await;
    assert_eq!(v.itens, vec!["t1 · Financeiro · conciliação"]);

    // Hipótese ligada ao território; o texto longo mantém a quebra de linha.
    passo(&reg, "nav-h", &[]).await;
    let v = passo(&reg, "novo", &[]).await;
    assert_eq!(v.selects["f-t"].0, vec!["t1 · Financeiro"]);
    let v = passo(
        &reg,
        "salvar",
        &[
            ("f-t", "t1 · Financeiro"),
            ("f-texto", "Conciliar leva\ndias"),
            ("f-status", "Aberta"),
        ],
    )
    .await;
    assert_eq!(v.itens, vec!["h2 [Aberta] Conciliar leva dias"]);
    let v = passo(&reg, "ab-h2", &[]).await;
    assert_eq!(v.campos["f-texto"], "Conciliar leva\ndias");

    // Entrevista: data inválida mantém a ficha e o que foi digitado.
    passo(&reg, "nav-i", &[]).await;
    let v = passo(&reg, "novo", &[]).await;
    assert_eq!(v.checks.get("hk-h2"), Some(&false));
    let ficha = [
        ("f-part", "P1"),
        ("f-emp", "Acme"),
        ("f-papel", "Responsável"),
        ("f-canal", "Indicação"),
        ("f-data", "12/11/2026"),
        ("f-fato", "Na última vez\nlevou 3 dias"),
        ("f-obs", "true"),
        ("hk-h2", "true"),
    ];
    let v = passo(&reg, "salvar", &ficha).await;
    assert!(v.textos.iter().any(|t| t.contains("AAAA-MM-DD")));
    assert_eq!(v.campos["f-fato"], "Na última vez\nlevou 3 dias");
    assert!(v.checks["hk-h2"]);
    let mut ok: Vec<(&str, &str)> = ficha
        .iter()
        .filter(|(k, _)| *k != "f-data")
        .copied()
        .collect();
    ok.push(("f-data", "2026-11-12"));
    let v = passo(&reg, "salvar", &ok).await;
    assert_eq!(v.itens, vec!["i3 · P1 · Acme · responsável · 2026-11-12"]);
    let v = passo(&reg, "ab-i3", &[]).await;
    assert_eq!(v.campos["f-fato"], "Na última vez\nlevou 3 dias");
    assert!(v.checks["hk-h2"]);
    assert!(v.checks["f-obs"]);
    assert_eq!(v.selects["f-canal"].1, "Indicação");
    passo(&reg, "cancelar", &[]).await;

    // O estado persiste entre execuções (instância nova a cada evento).
    let v = vista(&reg.dispatch(ID, &Event::Init).await.unwrap());
    assert_eq!(v.itens.len(), 1);

    // Território em uso não sai; remover a hipótese só desliga da entrevista.
    passo(&reg, "nav-t", &[]).await;
    passo(&reg, "rm-t1", &[]).await;
    let v = passo(&reg, "sim", &[]).await;
    assert!(v.textos.iter().any(|t| t.contains("Há hipóteses")));
    assert_eq!(v.itens.len(), 1);
    passo(&reg, "nav-h", &[]).await;
    let v = passo(&reg, "rm-h2", &[]).await;
    assert!(v.textos.iter().any(|t| t.contains("Remover")));
    let v = passo(&reg, "nao", &[]).await;
    assert_eq!(v.itens.len(), 1, "cancelar não remove");
    passo(&reg, "rm-h2", &[]).await;
    let v = passo(&reg, "sim", &[]).await;
    assert!(v.itens.is_empty());
    passo(&reg, "nav-i", &[]).await;
    let v = passo(&reg, "ab-i3", &[]).await;
    assert!(
        !v.checks.contains_key("hk-h2"),
        "hipótese removida some da ficha"
    );
    passo(&reg, "cancelar", &[]).await;

    // Agora o território e a entrevista podem sair.
    passo(&reg, "nav-t", &[]).await;
    passo(&reg, "rm-t1", &[]).await;
    assert!(passo(&reg, "sim", &[]).await.itens.is_empty());
    passo(&reg, "nav-i", &[]).await;
    passo(&reg, "rm-i3", &[]).await;
    assert!(passo(&reg, "sim", &[]).await.itens.is_empty());

    // Ids inexistentes ou malformados não quebram nada.
    for id in [
        "ab-zzz", "rm-t99", "rm-", "ab-", "pag-prox", "pag-ant", "sim", "xyz",
    ] {
        reg.dispatch(ID, &ev(id, &[])).await.unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn limits_pagination_and_worst_case_screen_still_validate() {
    let reg = registry("limits");

    // 5 territórios; o 6º é recusado com aviso.
    for i in 0..5 {
        novo_territorio(&reg, &format!("T{i}")).await;
    }
    passo(&reg, "novo", &[]).await;
    let v = passo(&reg, "salvar", &[("f-nome", "a mais")]).await;
    assert!(v.textos.iter().any(|t| t.contains("Limite de 5")));
    passo(&reg, "cancelar", &[]).await;

    // 20 hipóteses (o teto) e a ficha de entrevista com todas as caixas.
    passo(&reg, "nav-h", &[]).await;
    for i in 0..20 {
        passo(&reg, "novo", &[]).await;
        passo(
            &reg,
            "salvar",
            &[("f-t", "t1 · T0"), ("f-texto", &format!("hipótese {i}"))],
        )
        .await;
    }
    passo(&reg, "novo", &[]).await;
    let v = passo(&reg, "salvar", &[("f-t", "t1 · T0"), ("f-texto", "21ª")]).await;
    assert!(v.textos.iter().any(|t| t.contains("Limite de 20")));
    passo(&reg, "cancelar", &[]).await;

    // Pior caso da ficha: 3 textos de 4000 caracteres + 20 hipóteses marcadas.
    // O host valida a árvore (200 nós, 64 KiB): se passar do orçamento, o dispatch falha.
    let longo = "ç".repeat(3999) + "\n";
    let mut campos: Vec<(String, String)> = vec![
        ("f-part".into(), "P1".into()),
        ("f-fato".into(), longo.clone()),
        ("f-interp".into(), longo.clone()),
        ("f-duvida".into(), longo),
    ];
    for n in 1..=20 {
        // ids de hipótese: h6..h25 (t1..t5 foram criados antes)
        campos.push((format!("hk-h{}", 5 + n), "true".into()));
    }
    let refs: Vec<(&str, &str)> = campos
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    passo(&reg, "nav-i", &[]).await;
    passo(&reg, "novo", &[]).await;
    let v = passo(&reg, "salvar", &refs).await;
    assert_eq!(v.itens.len(), 1);
    let v = passo(&reg, "ab-i26", &[]).await;
    assert_eq!(v.checks.len(), 20 + 1, "20 hipóteses + observação");
    assert_eq!(v.campos["f-fato"].chars().count(), 3999);
    passo(&reg, "cancelar", &[]).await;

    // Paginação: 16 entrevistas -> 2 páginas, e as duas telas passam na validação.
    for i in 0..15 {
        passo(&reg, "novo", &[]).await;
        passo(&reg, "salvar", &[("f-part", &format!("P{i}"))]).await;
    }
    let v = passo(&reg, "nav-i", &[]).await;
    assert_eq!(v.itens.len(), 15);
    assert!(v.botoes.contains(&"pag-prox".to_string()));
    assert!(v.textos.iter().any(|t| t == "Página 1 de 2"));
    let v = passo(&reg, "pag-prox", &[]).await;
    assert_eq!(v.itens.len(), 1);
    assert!(v.botoes.contains(&"pag-ant".to_string()));
    let v = passo(&reg, "pag-prox", &[]).await;
    assert_eq!(v.itens.len(), 1, "além da última página fica na última");
}

#[tokio::test(flavor = "current_thread")]
async fn newer_schema_is_read_only_and_roteiro_is_shown() {
    let reg = registry("ro");
    // Backup com `meta` de uma versão mais nova do app.
    let meta = br#"{"schema":99,"seq":0}"#;
    let hex: String = meta.iter().map(|b| format!("{b:02x}")).collect();
    let backup = format!(
        r#"{{"format":"superapp-backup-1","app_id":"{ID}","app_version":"0.1.0","created_at":1,"entries":[{{"key":"meta","value_hex":"{hex}"}}]}}"#
    );
    reg.import_data(ID, backup.as_bytes()).unwrap();

    let v = vista(&reg.dispatch(ID, &Event::Init).await.unwrap());
    assert!(v.textos.iter().any(|t| t.contains("somente leitura")));
    let v = passo(&reg, "novo", &[]).await;
    assert!(v.textos.iter().any(|t| t.contains("somente leitura")));
    assert!(
        !v.campos.contains_key("f-nome"),
        "não abre ficha em somente leitura"
    );

    // O roteiro é texto fixo e passa na validação da UI.
    let n = reg.dispatch(ID, &ev("nav-r", &[])).await.unwrap();
    let json = serde_json::to_string(&n).unwrap();
    assert!(json.contains("última vez que você precisou"));
}

#[tokio::test(flavor = "current_thread")]
async fn index_repair_restores_orphans_and_drops_dangling_ids() {
    let reg = registry("repair");
    novo_territorio(&reg, "A").await;
    novo_territorio(&reg, "B").await;
    let backup = reg.export_data(ID).unwrap();
    let mut doc: serde_json::Value = serde_json::from_slice(&backup).unwrap();
    let hex = |s: &str| -> String { s.bytes().map(|b| format!("{b:02x}")).collect() };
    let entries = doc["entries"].as_array_mut().unwrap();
    for e in entries.iter_mut() {
        // Índice com um id sem registro (t9) e sem o t2 (órfão).
        if e["key"] == "idx:t" {
            e["value_hex"] = hex(r#"["t1","t9"]"#).into();
        }
    }
    reg.import_data(ID, serde_json::to_string(&doc).unwrap().as_bytes())
        .unwrap();
    let v = vista(&reg.dispatch(ID, &Event::Init).await.unwrap());
    assert_eq!(v.itens, vec!["t1 · A · fechamento", "t2 · B · fechamento"]);
}

fn tem(v: &Vista, trecho: &str) -> bool {
    v.listas
        .iter()
        .chain(v.textos.iter())
        .any(|t| t.contains(trecho))
}

#[tokio::test(flavor = "current_thread")]
async fn evidence_decision_and_panel_flow() {
    let reg = registry("d3");
    passo(&reg, "nav-t", &[]).await;
    passo(&reg, "novo", &[]).await;
    passo(
        &reg,
        "salvar",
        &[
            ("f-nome", "Financeiro"),
            ("f-processo", "fechamento"),
            ("f-alternativa", "planilhas"),
        ],
    )
    .await; // t1
    passo(&reg, "nav-h", &[]).await;
    passo(&reg, "novo", &[]).await;
    passo(
        &reg,
        "salvar",
        &[("f-t", "t1 · Financeiro"), ("f-texto", "Fechar leva dias")],
    )
    .await; // h2, sem "evidência contrária" definida
    for (p, emp, papel) in [
        ("P1", "Acme", "Responsável"),
        ("P2", " ACME ", "Executor"),
        ("P3", "Beta", "Comprador"),
    ] {
        passo(&reg, "nav-i", &[]).await;
        passo(&reg, "novo", &[]).await;
        passo(
            &reg,
            "salvar",
            &[
                ("f-part", p),
                ("f-emp", emp),
                ("f-papel", papel),
                ("hk-h2", "true"),
            ],
        )
        .await;
    } // i3, i4, i5

    // Validações da ficha de evidência (a ficha continua aberta, com o que foi digitado).
    passo(&reg, "nav-v", &[]).await;
    passo(&reg, "novo", &[]).await;
    let h = "h2 · Fechar leva dias";
    let v = passo(&reg, "salvar", &[("f-h", h), ("f-resumo", "x")]).await;
    assert!(tem(&v, "Informe a fonte"));
    assert_eq!(v.campos["f-resumo"], "x");
    let v = passo(&reg, "salvar", &[("f-h", h), ("f-fonte", "i3")]).await;
    assert!(tem(&v, "Resuma"));
    let f2 = "2 · Entrevista detalhada";
    let v = passo(
        &reg,
        "salvar",
        &[
            ("f-h", h),
            ("f-fonte", "i3"),
            ("f-resumo", "levou 3 dias"),
            ("f-forca", f2),
        ],
    )
    .await;
    assert_eq!(
        v.itens,
        vec!["v6 · h2 · F2 · a favor · Acme · levou 3 dias"]
    );
    passo(&reg, "novo", &[]).await;
    passo(
        &reg,
        "salvar",
        &[
            ("f-h", h),
            ("f-fonte", "i4"),
            ("f-resumo", "refaz tudo"),
            ("f-forca", f2),
        ],
    )
    .await;
    passo(&reg, "novo", &[]).await;
    passo(
        &reg,
        "salvar",
        &[
            ("f-h", h),
            ("f-fonte", "i5"),
            ("f-resumo", "tentou outra ferramenta"),
            ("f-forca", "4 · Tentativa anterior de solução"),
        ],
    )
    .await;
    // A empresa e o papel vêm da entrevista citada como fonte.
    let v = passo(&reg, "ab-v7", &[]).await;
    assert_eq!(v.campos["f-emp"], "ACME");
    assert_eq!(v.selects["f-forca"].1, f2);
    passo(&reg, "cancelar", &[]).await;

    // Painel: "Acme" e " ACME " são a mesma empresa (2 independentes em 3 falas).
    let v = passo(&reg, "nav-p", &[]).await;
    assert!(tem(
        &v,
        "a favor: 3 (empresas indep.: 2; papéis: 3; força máx.: 4) · contra: 0 (força máx.: 0)"
    ));
    assert!(tem(&v, "⚠ sem evidência contrária definida"));
    assert!(tem(&v, "✗ Episódios em empresas independentes: 2"));
    assert!(tem(&v, "✓ Alternativa atual compreendida"));
    assert!(tem(&v, "✓ Mobilização para mudar"));
    assert!(tem(&v, "✗ Acesso a quem pode testar"));
    assert!(tem(&v, "✓ Contradições revisadas"));
    assert!(tem(&v, "Nenhuma decisão registrada"));

    // Evidência contrária abre uma contradição até haver uma decisão posterior sobre a hipótese.
    passo(&reg, "nav-v", &[]).await;
    passo(&reg, "novo", &[]).await;
    passo(
        &reg,
        "salvar",
        &[
            ("f-h", h),
            ("f-fonte", "https://exemplo.com/post"),
            ("f-resumo", "outro time resolve em horas"),
            ("f-forca", "3 · Observação autorizada"),
            ("f-contra", "true"),
            ("f-emp", "Gama"),
        ],
    )
    .await; // v9
    let v = passo(&reg, "nav-p", &[]).await;
    assert!(tem(&v, "contra: 1 (força máx.: 3)"));
    assert!(tem(&v, "⚠ contradição aberta"));
    assert!(tem(&v, "✗ Contradições revisadas"));

    passo(&reg, "nav-d", &[]).await;
    passo(&reg, "novo", &[]).await;
    let ficha = [
        ("f-t", "t1 · Financeiro"),
        ("f-esc", "Restringir o segmento"),
        ("hk-h2", "true"),
    ];
    let v = passo(&reg, "salvar", &ficha).await;
    assert!(tem(&v, "Explique o porquê"));
    let mut ok = ficha.to_vec();
    ok.push(("f-porque", "Foco em responsáveis"));
    let v = passo(&reg, "salvar", &ok).await;
    assert_eq!(v.itens.len(), 1);
    assert!(v.itens[0].starts_with("d10 · t1 · Restringir o segmento · Foco em responsáveis"));
    let v = passo(&reg, "nav-p", &[]).await;
    assert!(!tem(&v, "⚠ contradição aberta"));
    assert!(tem(&v, "✓ Contradições revisadas"));
    assert!(tem(
        &v,
        "Última decisão: Restringir o segmento (d10): Foco em responsáveis"
    ));

    // Hipótese com evidências não sai; remover a entrevista leva junto as evidências que a citam.
    passo(&reg, "nav-h", &[]).await;
    passo(&reg, "rm-h2", &[]).await;
    let v = passo(&reg, "sim", &[]).await;
    assert!(tem(&v, "Há evidências ligadas"));
    assert_eq!(v.itens.len(), 1);
    passo(&reg, "nav-i", &[]).await;
    let v = passo(&reg, "rm-i3", &[]).await;
    assert!(tem(&v, "evidências que a citam"));
    passo(&reg, "sim", &[]).await;
    let v = passo(&reg, "nav-v", &[]).await;
    assert_eq!(v.itens.len(), 3, "v6 saiu junto com a entrevista i3");
    assert!(v.itens.iter().all(|i| !i.starts_with("v6 ")));

    // Sem a decisão, a contradição volta a aparecer.
    passo(&reg, "nav-d", &[]).await;
    passo(&reg, "rm-d10", &[]).await;
    passo(&reg, "sim", &[]).await;
    let v = passo(&reg, "nav-p", &[]).await;
    assert!(tem(&v, "⚠ contradição aberta"));
    assert!(tem(&v, "a favor: 2 (empresas indep.: 2"));
}
