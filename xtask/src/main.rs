//! Orquestrador de tarefas do monorepo (ADR-008): `cargo xtask <comando>`.
use serde_json::Value;
use std::process::{exit, Command};
use std::time::{Duration, Instant};

fn main() {
    let task = std::env::args().nth(1).unwrap_or_default();
    let ok = match task.as_str() {
        "fmt" => run("cargo", &["fmt", "--all", "--check"]),
        "lint" => lint(),
        "test" => run("cargo", &["test", "--workspace"]),
        "ci" => run("cargo", &["fmt", "--all", "--check"]) && lint() && test(),
        "smoke" => smoke(),
        "bench" => bench(),
        "keygen" => keygen(),
        "pack" => pack_cmd(),
        "verify" => verify_cmd(),
        "catalog" => catalog_cmd(),
        "new-app" => new_app_cmd(),
        "build-app" => build_app_cmd(),
        _ => {
            eprintln!("uso: cargo xtask <fmt|lint|test|ci|smoke|bench|keygen|pack|verify|catalog|new-app|build-app>");
            exit(2);
        }
    };
    if !ok {
        exit(1);
    }
}

fn lint() -> bool {
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )
}

fn test() -> bool {
    run("cargo", &["test", "--workspace"])
}

fn run(program: &str, args: &[&str]) -> bool {
    println!("> {program} {}", args.join(" "));
    Command::new(program)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Smoke test do app real: sobe o binário em modo smoke (duas janelas), espera o relatório
/// e confere as decisões de segurança (capabilities, hook de isolamento, auditoria).
fn smoke() -> bool {
    if !run(
        "cargo",
        &["build", "-p", "superapp", "--features", "custom-protocol"],
    ) {
        return false;
    }
    let exe = if cfg!(windows) {
        "target/debug/superapp.exe"
    } else {
        "target/debug/superapp"
    };
    let out = std::env::temp_dir().join(format!("superapp-smoke-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let Some((data_dir, sapp)) = prepare_smoke_package() else {
        return false;
    };
    println!("> {exe} (SUPERAPP_SMOKE_OUT={})", out.display());
    let Ok(mut child) = Command::new(exe)
        .env("SUPERAPP_SMOKE_OUT", &out)
        .env("SUPERAPP_DATA_DIR", &data_dir)
        .env("SUPERAPP_SMOKE_SAPP", &sapp)
        .spawn()
    else {
        eprintln!("não consegui iniciar {exe}");
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            eprintln!("smoke: timeout de 90 s sem relatório");
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let Ok(text) = std::fs::read_to_string(&out) else {
        eprintln!("smoke: o app saiu sem gravar o relatório");
        return false;
    };
    let _ = std::fs::remove_file(&out);
    let Ok(report) = serde_json::from_str::<Value>(&text) else {
        eprintln!("smoke: relatório inválido: {text}");
        return false;
    };
    check_report(&report)
}

fn f_unknown(f: &Value) -> &Value {
    &f["unknown"]
}

/// Compila o mini-app `clicker`, assina com uma chave efêmera e prepara o diretório de dados
/// do app (com a lista de chaves confiáveis). Devolve (diretório de dados, caminho do .sapp).
fn prepare_smoke_package() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let ok = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir("miniapps/clicker")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("não compilei o clicker (rustup target add wasm32-wasip2)");
        return None;
    }
    let wasm = std::fs::read("miniapps/clicker/target/wasm32-wasip2/release/clicker.wasm").ok()?;
    let base = std::env::temp_dir().join(format!("superapp-smoke-data-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let data = base.join("data");
    std::fs::create_dir_all(&data).ok()?;
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).ok()?;
    let key = ed25519_dalek::SigningKey::from_bytes(&seed);
    let manifest = superapp_package::Manifest {
        schema: 1,
        id: "clicker".into(),
        name: "Clicker".into(),
        version: "0.1.0".into(),
        permissions: vec![superapp_package::Permission::Storage],
        wasm_sha256: String::new(),
        limits: None,
    };
    let bytes = superapp_package::pack(&key, &manifest, &wasm).ok()?;
    let sapp = base.join("clicker.sapp");
    std::fs::write(&sapp, bytes).ok()?;
    std::fs::write(
        data.join("trusted-keys.txt"),
        superapp_package::to_hex(key.verifying_key().as_bytes()),
    )
    .ok()?;
    Some((data, sapp))
}

fn check_report(report: &Value) -> bool {
    let r = &report["results"];
    let code = |v: &Value| v["error"]["code"].as_str().unwrap_or("").to_owned();
    let mut checks: Vec<(&str, bool)> = vec![
        (
            "main: app.echo permitido",
            r["echo_ok"]["ok"] == true && r["echo_ok"]["value"]["text"] == "oi",
        ),
        (
            "args inválidos → invalid_args",
            code(&r["echo_invalid"]) == "invalid_args",
        ),
        (
            "comando inexistente → not_found",
            code(&r["unknown"]) == "not_found",
        ),
        (
            "sem capability → permission_denied",
            code(&r["privileged"]) == "permission_denied",
        ),
        (
            "hook: nome inválido barrado",
            code(&r["hook_bad_name"]) == "permission_denied",
        ),
        (
            "hook: plugin:fs barrado",
            code(&r["hook_plugin"]) == "permission_denied",
        ),
        (
            "hook: args > 64 KB barrados",
            code(&r["hook_big_args"]) == "permission_denied",
        ),
        (
            "untrusted: app.info negado",
            code(&r["untrusted"]["info"]) == "permission_denied",
        ),
        (
            "untrusted: app.echo negado",
            code(&r["untrusted"]["echo"]) == "permission_denied",
        ),
    ];

    let ap = &r["apps"];
    let ui_text = |v: &Value| {
        v["value"]["children"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_owned()
    };
    checks.extend([
        (
            "apps: pacote arrastado fica pendente com preview correto",
            ap["pending"] == 1
                && ap["preview"]["id"] == "clicker"
                && ap["preview"]["is_update"] == false
                && ap["preview"]["requested"] == serde_json::json!(["storage"])
                && ap["preview"]["added"] == serde_json::json!(["storage"]),
        ),
        (
            "apps: token desconhecido → not_found",
            code(&ap["bad_token"]) == "not_found",
        ),
        (
            "apps: permissão inexistente → invalid_args",
            code(&ap["bad_perm"]) == "invalid_args",
        ),
        (
            "apps: aprovação além do manifesto → invalid_args",
            code(&ap["exceed"]) == "invalid_args",
        ),
        (
            "apps: instala com consentimento",
            ap["install"]["ok"] == true && ap["install"]["value"]["enabled"] == true,
        ),
        (
            "apps: UI declarativa inicial e após 2 cliques",
            ui_text(&ap["init"]) == "Cliques: 0" && ui_text(&ap["click"]) == "Cliques: 2",
        ),
        (
            "apps: id de evento inválido barrado",
            code(&ap["bad_event"]) == "invalid_args",
        ),
        (
            "apps: suspenso não executa; retomado executa com dados preservados",
            ap["suspend"]["ok"] == true
                && code(&ap["suspended_run"]) == "permission_denied"
                && ui_text(&ap["resumed"]) == "Cliques: 2",
        ),
        (
            "apps: remoção limpa lista, pendências e execução",
            ap["list"]["value"].as_array().map(Vec::len) == Some(1)
                && ap["uninstall"]["ok"] == true
                && ap["after"]["value"].as_array().map(Vec::len) == Some(0)
                && code(&ap["gone"]) == "not_found"
                && ap["pending_after"] == 0,
        ),
        (
            "apps: janela sem capability não lista nem executa",
            code(&r["untrusted"]["apps"]) == "permission_denied"
                && code(&r["untrusted"]["appsRun"]) == "permission_denied",
        ),
        (
            "updates: janela sem capability não verifica nem instala atualização",
            code(&r["untrusted"]["upd"]) == "permission_denied"
                && code(&r["untrusted"]["updInstall"]) == "permission_denied",
        ),
        (
            "store: janela sem capability não lê catálogo nem baixa",
            code(&r["untrusted"]["storeCat"]) == "permission_denied"
                && code(&r["untrusted"]["storeDl"]) == "permission_denied",
        ),
        (
            "diag: janela sem capability não gera relatório",
            code(&r["untrusted"]["diag"]) == "permission_denied",
        ),
        (
            "trust: janela sem capability não vê nem muda as chaves confiáveis",
            ["trustList", "trustAdd", "trustRm"]
                .iter()
                .all(|k| code(&r["untrusted"][*k]) == "permission_denied"),
        ),
        (
            "data/apps.pick/restart: janela sem capability não exporta, importa, escolhe arquivo nem reinicia",
            ["dataExp", "dataImp", "appsPick", "restart"]
                .iter()
                .all(|k| code(&r["untrusted"][*k]) == "permission_denied"),
        ),
        (
            "ui: valores de campos no clique são validados e o formato antigo segue valendo",
            ap["click_values"]["ok"] == true
                && code(&ap["bad_values"]) == "invalid_args"
                && code(&ap["bad_value_text"]) == "invalid_args"
                && code(&ap["unknown_event_field"]) == "invalid_args",
        ),
        (
            "trust: lista funciona na janela principal; entrada ruim ou ausente é recusada",
            ap["trust_list"]["ok"] == true
                && code(&ap["trust_bad"]) == "invalid_args"
                && code(&ap["trust_missing"]) == "not_found",
        ),
        (
            "data: backup de mini-app inexistente falha sem abrir diálogo",
            code(&ap["export_missing"]) == "not_found",
        ),
    ]);

    let m = &r["mfe"];
    println!("  mfe: {m}");
    checks.extend([
        (
            "mfe: abre a e b (id fora da lista recusado)",
            m["open_a"]["ok"] == true
                && m["open_b"]["ok"] == true
                && code(&m["open_bad"]) == "invalid_args",
        ),
        (
            "mfe: a e b ficam prontos",
            m["ready"] == serde_json::json!(["a", "b"]),
        ),
        (
            "mfe: com A travado, o shell ainda fala com o host",
            m["shell_rpc"]["ok"] == true && m["shell_rpc_ms"].as_u64().unwrap_or(u64::MAX) < 1000,
        ),
        (
            "mfe: com A travado, B ainda responde no BroadcastChannel",
            m["pong_from"] == serde_json::json!(["b"]),
        ),
    ]);

    let b = &r["bytes"];
    println!("  ipc_bytes (com Isolation): gen 10 MiB {} ms", b["gen_ms"]);
    println!("  bytes envio (ms): {}", b["up"]);
    let f = &b["blob"];
    println!(
        "  sapp://: gen 10 MiB {} ms, echo 10 MiB {} ms, envio 10 MiB {} ms",
        f["gen_ms"], f["echo_ms"], f["up_ms"]
    );
    checks.extend([
        (
            "sapp: round-trip 1 MiB e 10 MiB íntegros",
            f["echo_1mib"] == true && f["gen_ok"] == true && f["echo10_ok"] == true,
        ),
        (
            "sapp: desconhecido → not_found",
            code(f_unknown(f)) == "not_found",
        ),
        (
            "sapp: entrada > 16 MiB → invalid_args",
            code(&f["over_in"]) == "invalid_args",
        ),
        (
            "sapp: saída > 16 MiB → internal",
            code(&f["over_out"]) == "internal",
        ),
        (
            "sapp: argumento malformado → invalid_args",
            code(&f["bad_gen"]) == "invalid_args",
        ),
        (
            "sapp: untrusted negado",
            code(&r["untrusted"]["blob"]) == "permission_denied",
        ),
    ]);
    checks.extend([
        (
            "bytes: round-trip 4 B e 1 MiB íntegros",
            b["echo_small"] == true && b["echo_1mib"] == true,
        ),
        ("bytes: 10 MiB gerados íntegros", b["gen_ok"] == true),
        (
            "bytes: comando desconhecido → not_found",
            code(&b["unknown"]) == "not_found",
        ),
        (
            "bytes: entrada > 1 MiB barrada pelo hook",
            code(&b["over_in"]) == "permission_denied",
        ),
        (
            "bytes: saída > 16 MiB → internal",
            code(&b["over_out"]) == "internal",
        ),
        (
            "bytes: argumento malformado → invalid_args",
            code(&b["bad_gen"]) == "invalid_args",
        ),
        (
            "bytes: untrusted negado",
            code(&r["untrusted"]["bytes"]) == "permission_denied",
        ),
    ]);

    let audit = report["audit"].as_array().cloned().unwrap_or_default();
    let has = |window: &str, command: &str, decision: &str| {
        audit
            .iter()
            .any(|e| e["window"] == window && e["command"] == command && e["decision"] == decision)
    };
    checks.extend([
        (
            "auditoria: allowed main/app.echo",
            has("main", "app.echo", "Allowed"),
        ),
        (
            "auditoria: negado untrusted/app.info",
            has("untrusted", "app.info", "DeniedNoCapability"),
        ),
        (
            "auditoria: comando desconhecido",
            has("main", "nao.existe", "DeniedUnknownCommand"),
        ),
        (
            "auditoria: rejeição do hook (plugin)",
            has("main", "plugin:fs|read_text_file", "DeniedByHook"),
        ),
        (
            "auditoria: rejeição do hook (nome inválido, sanitizado)",
            has("main", "Bad?Name?", "DeniedByHook"),
        ),
    ]);

    let mut ok = true;
    for (name, pass) in &checks {
        println!("  {} {name}", if *pass { "ok  " } else { "FALHA" });
        ok &= *pass;
    }
    if !ok {
        eprintln!("relatório completo: {report}");
    }
    ok
}

/// Benchmark de release: tamanho do binário, tempo até a UI carregar e memória (host + WebView).
/// Imprime JSON e, se `GITHUB_STEP_SUMMARY` existir, anexa uma tabela Markdown.
fn bench() -> bool {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    if !run(
        "cargo",
        &[
            "build",
            "-p",
            "superapp",
            "--release",
            "--features",
            "custom-protocol",
        ],
    ) {
        return false;
    }
    let exe = if cfg!(windows) {
        "target/release/superapp.exe"
    } else {
        "target/release/superapp"
    };
    let size_mb = std::fs::metadata(exe)
        .map(|m| m.len() as f64 / 1_048_576.0)
        .unwrap_or(0.0);
    let out = std::env::temp_dir().join(format!("superapp-bench-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let Ok(mut child) = Command::new(exe).env("SUPERAPP_BENCH_OUT", &out).spawn() else {
        eprintln!("não consegui iniciar {exe}");
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    while !out.exists() {
        if Instant::now() > deadline {
            let _ = child.kill();
            eprintln!("bench: timeout sem a UI avisar que carregou");
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Deixa a UI assentar e soma a memória do app e de todos os descendentes (WebView).
    std::thread::sleep(Duration::from_secs(3));
    let startup_ms = std::fs::read_to_string(&out)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["startup_ms"].as_u64())
        .unwrap_or(0);
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let root = Pid::from_u32(child.id());
    let mut tree = vec![root];
    loop {
        let before = tree.len();
        for (pid, p) in sys.processes() {
            // No Linux o sysinfo também lista threads como processos: ignora-as.
            if p.thread_kind().is_some() {
                continue;
            }
            if p.parent().is_some_and(|pp| tree.contains(&pp)) && !tree.contains(pid) {
                tree.push(*pid);
            }
        }
        if tree.len() == before {
            break;
        }
    }
    let rss = |pids: &[Pid]| -> f64 {
        pids.iter()
            .filter_map(|p| sys.process(*p))
            .map(|p| p.memory() as f64)
            .sum::<f64>()
            / 1_048_576.0
    };
    let host_mb = rss(&[root]);
    let total_mb = rss(&tree);
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&out);

    let report = serde_json::json!({
        "os": std::env::consts::OS,
        "binary_mb": (size_mb * 10.0).round() / 10.0,
        "startup_ms": startup_ms,
        "host_rss_mb": (host_mb * 10.0).round() / 10.0,
        "total_rss_mb": (total_mb * 10.0).round() / 10.0,
        "processes": tree.len(),
    });
    println!("bench: {report}");
    if let Ok(path) = std::env::var("GITHUB_STEP_SUMMARY") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
        {
            let _ = writeln!(
                f,
                "### Benchmark ({})

| métrica | valor |
|---|---|
| binário release | {} MB |
| UI pronta | {} ms |
| RAM host | {} MB |
| RAM total (soma de RSS de host + WebView, {} processos; superestima: páginas compartilhadas contam várias vezes) | {} MB |
",
                report["os"].as_str().unwrap_or("?"),
                report["binary_mb"],
                startup_ms,
                report["host_rss_mb"],
                report["processes"],
                report["total_rss_mb"]
            );
        }
    }
    // Só falha se a UI nunca carregou. Tempo alto vira aviso: em CI sem GPU (xvfb) o
    // WebKit renderiza por software e leva dezenas de segundos sem refletir o uso real.
    if startup_ms > 15_000 {
        println!("bench: AVISO: UI pronta em {startup_ms} ms (> 15 s)");
    }
    startup_ms > 0
}

// ---- Pacotes de mini-app (ADR-010) -------------------------------------------------------

/// `cargo xtask keygen <arquivo>`: cria uma chave privada (semente em hex) e imprime a pública.
/// Nunca sobrescreve. Arquivos `*.key` são ignorados pelo git: não os versione.
fn keygen() -> bool {
    let Some(path) = std::env::args().nth(2) else {
        eprintln!("uso: cargo xtask keygen <arquivo.key>");
        return false;
    };
    if std::path::Path::new(&path).exists() {
        eprintln!("{path} já existe; recuso sobrescrever");
        return false;
    }
    let mut seed = [0u8; 32];
    if getrandom::fill(&mut seed).is_err() {
        eprintln!("sem fonte de aleatoriedade do SO");
        return false;
    }
    let key = ed25519_dalek::SigningKey::from_bytes(&seed);
    if std::fs::write(&path, superapp_package::to_hex(&seed)).is_err() {
        eprintln!("não consegui gravar {path}");
        return false;
    }
    println!("chave privada: {path} (guarde com cuidado)");
    println!(
        "chave pública (cole em trusted-keys.txt): {}",
        superapp_package::to_hex(key.verifying_key().as_bytes())
    );
    true
}

fn load_key(path: &str) -> Option<ed25519_dalek::SigningKey> {
    let text = std::fs::read_to_string(path).ok()?;
    let seed = superapp_package::parse_hex32(text.trim())?;
    Some(ed25519_dalek::SigningKey::from_bytes(&seed))
}

/// `cargo xtask pack <app.wasm> <manifest.json> <chave.key> <saida.sapp>`
fn pack_cmd() -> bool {
    let a: Vec<String> = std::env::args().skip(2).collect();
    let [wasm, manifest, key, out] = a.as_slice() else {
        eprintln!("uso: cargo xtask pack <app.wasm> <manifest.json> <chave.key> <saida.sapp>");
        return false;
    };
    pack_files(wasm, manifest, key, out)
}

fn pack_files(wasm: &str, manifest: &str, key: &str, out: &str) -> bool {
    let (Ok(wasm), Ok(mtext)) = (std::fs::read(wasm), std::fs::read_to_string(manifest)) else {
        eprintln!("não consegui ler o wasm ou o manifesto");
        return false;
    };
    let Some(key) = load_key(key) else {
        eprintln!("chave privada inválida (esperado 64 hex)");
        return false;
    };
    let manifest: superapp_package::Manifest = match serde_json::from_str(&mtext) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("manifesto inválido: {e}");
            return false;
        }
    };
    match superapp_package::pack(&key, &manifest, &wasm) {
        Ok(bytes) => {
            if std::fs::write(out, &bytes).is_err() {
                eprintln!("não consegui gravar {out}");
                return false;
            }
            println!(
                "{out}: {} bytes, assinado por {}",
                bytes.len(),
                superapp_package::to_hex(key.verifying_key().as_bytes())
            );
            true
        }
        Err(e) => {
            eprintln!("falha ao empacotar: {e}");
            false
        }
    }
}

/// `cargo xtask verify <pacote.sapp> <trusted-keys.txt>`
fn verify_cmd() -> bool {
    let a: Vec<String> = std::env::args().skip(2).collect();
    let [pkg, trust] = a.as_slice() else {
        eprintln!("uso: cargo xtask verify <pacote.sapp> <trusted-keys.txt>");
        return false;
    };
    let (Ok(bytes), Ok(ttext)) = (std::fs::read(pkg), std::fs::read_to_string(trust)) else {
        eprintln!("não consegui ler os arquivos");
        return false;
    };
    let store = match superapp_package::TrustStore::from_hex_lines(&ttext) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return false;
        }
    };
    match superapp_package::verify(&bytes, &store) {
        Ok(v) => {
            println!(
                "OK: {} {} ({} bytes de wasm)",
                v.manifest.id,
                v.manifest.version,
                v.wasm.len()
            );
            println!("permissões: {:?}", v.manifest.permissions);
            true
        }
        Err(e) => {
            eprintln!("REPROVADO: {e}");
            false
        }
    }
}

/// `cargo xtask catalog <chave.key> <entradas.json> <saida.json> [dias]`
/// `entradas.json`: lista de `{ id, name, version, url, file }`; `file` é o `.sapp` local, de onde
/// saem o SHA-256 e o tamanho. O catálogo sai assinado e com validade (padrão 90 dias).
fn catalog_cmd() -> bool {
    let a: Vec<String> = std::env::args().skip(2).collect();
    if a.len() < 3 || a.len() > 4 {
        eprintln!("uso: cargo xtask catalog <chave.key> <entradas.json> <saida.json> [dias]");
        return false;
    }
    let days: u64 = a.get(3).and_then(|d| d.parse().ok()).unwrap_or(90);
    let Some(key) = load_key(&a[0]) else {
        eprintln!("chave privada inválida (esperado 64 hex)");
        return false;
    };
    let Ok(text) = std::fs::read_to_string(&a[1]) else {
        eprintln!("não consegui ler {}", a[1]);
        return false;
    };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text) else {
        eprintln!("entradas.json precisa ser uma lista");
        return false;
    };
    let base = std::path::Path::new(&a[1])
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let mut entries = Vec::new();
    for it in &items {
        let field = |k: &str| it[k].as_str().map(str::to_string);
        let (Some(id), Some(name), Some(version), Some(url), Some(file)) = (
            field("id"),
            field("name"),
            field("version"),
            field("url"),
            field("file"),
        ) else {
            eprintln!("entrada sem id/name/version/url/file: {it}");
            return false;
        };
        let Ok(bytes) = std::fs::read(base.join(&file)) else {
            eprintln!("não consegui ler {file}");
            return false;
        };
        entries.push(superapp_package::catalog::Entry {
            id,
            name,
            version,
            url,
            sha256: superapp_package::sha256_hex(&bytes),
            size: bytes.len() as u64,
        });
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cat = superapp_package::catalog::Catalog {
        format: 1,
        issued_at: now,
        expires_at: now + days * 86_400,
        entries,
    };
    let signed = match superapp_package::catalog::sign(&key, &cat) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("falha ao assinar: {e}");
            return false;
        }
    };
    // Confere o próprio resultado antes de gravar: nunca publicar catálogo que não verifica.
    let trust = superapp_package::TrustStore::new(vec![*key.verifying_key().as_bytes()]);
    if let Err(e) = superapp_package::catalog::verify(&signed, &trust, now) {
        eprintln!("o catálogo gerado não passou na verificação: {e}");
        return false;
    }
    if std::fs::write(&a[2], &signed).is_err() {
        eprintln!("não consegui gravar {}", a[2]);
        return false;
    }
    println!(
        "{}: {} entradas, válido por {days} dias, assinado por {}",
        a[2],
        cat.entries.len(),
        superapp_package::to_hex(key.verifying_key().as_bytes())
    );
    true
}

/// `cargo xtask new-app <id> [Nome]`: cria `miniapps/<id>/` a partir do app `tarefas` (que é
/// testado de ponta a ponta), com Cargo.toml e manifesto já ajustados ao novo id.
fn new_app_cmd() -> bool {
    let a: Vec<String> = std::env::args().skip(2).collect();
    let Some(id) = a.first() else {
        eprintln!("uso: cargo xtask new-app <id> [Nome]   (id: letras minúsculas, dígitos e '-')");
        return false;
    };
    if !superapp_package::valid_app_id(id) {
        eprintln!("id inválido: use [a-z0-9-], de 1 a 32 caracteres");
        return false;
    }
    let name = a.get(1).cloned().unwrap_or_else(|| id.clone());
    let dir = std::path::Path::new("miniapps").join(id);
    if dir.exists() {
        eprintln!("{} já existe; recuso sobrescrever", dir.display());
        return false;
    }
    let cargo = include_str!("../../miniapps/tarefas/Cargo.toml").replacen(
        "name = \"tarefas\"",
        &format!("name = \"{id}\""),
        1,
    );
    let manifest = serde_json::json!({
        "schema": 1, "id": id, "name": name, "version": "0.1.0", "permissions": ["storage"]
    });
    let files: [(&str, String); 3] = [
        ("Cargo.toml", cargo),
        (
            "manifest.json",
            serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n",
        ),
        (
            "src/lib.rs",
            include_str!("../../miniapps/tarefas/src/lib.rs").to_string(),
        ),
    ];
    if std::fs::create_dir_all(dir.join("src")).is_err() {
        eprintln!("não consegui criar {}", dir.display());
        return false;
    }
    for (path, content) in files {
        if std::fs::write(dir.join(path), content).is_err() {
            eprintln!("não consegui gravar {path}");
            return false;
        }
    }
    println!("criado miniapps/{id}");
    println!("  1. edite miniapps/{id}/src/lib.rs (e as permissões em manifest.json)");
    println!("  2. cargo xtask build-app {id} <sua-chave.key>   # compila e empacota");
    println!(
        "  3. arraste miniapps/{id}/{id}.sapp para o Superapp (ou use \"Instalar de um arquivo\")"
    );
    true
}

/// `cargo xtask build-app <id> <chave.key> [saida.sapp]`: compila `miniapps/<id>` (Rust) para
/// wasm32-wasip2 e empacota/assina com o manifesto do diretório. Saída padrão: `<id>.sapp` ali.
fn build_app_cmd() -> bool {
    let a: Vec<String> = std::env::args().skip(2).collect();
    if a.len() < 2 || a.len() > 3 {
        eprintln!("uso: cargo xtask build-app <id> <chave.key> [saida.sapp]");
        return false;
    }
    let id = &a[0];
    if !superapp_package::valid_app_id(id) {
        eprintln!("id inválido");
        return false;
    }
    let dir = std::path::Path::new("miniapps").join(id);
    if !dir.join("Cargo.toml").is_file() || !dir.join("manifest.json").is_file() {
        eprintln!(
            "{} precisa de Cargo.toml e manifest.json (só apps Rust; JavaScript: veja build.mjs)",
            dir.display()
        );
        return false;
    }
    let built = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir(&dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !built {
        eprintln!("a compilação falhou (uma vez: rustup target add wasm32-wasip2)");
        return false;
    }
    let wasm = dir
        .join("target/wasm32-wasip2/release")
        .join(format!("{}.wasm", id.replace('-', "_")));
    let out = a.get(2).cloned().unwrap_or_else(|| {
        dir.join(format!("{id}.sapp"))
            .to_string_lossy()
            .into_owned()
    });
    pack_files(
        &wasm.to_string_lossy(),
        &dir.join("manifest.json").to_string_lossy(),
        &a[1],
        &out,
    )
}
