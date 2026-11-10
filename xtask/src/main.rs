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
        _ => {
            eprintln!("uso: cargo xtask <fmt|lint|test|ci|smoke|bench>");
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

/// Smoke test do app real: sobe o binÃ¡rio em modo smoke (duas janelas), espera o relatÃ³rio
/// e confere as decisÃµes de seguranÃ§a (capabilities, hook de isolamento, auditoria).
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
    println!("> {exe} (SUPERAPP_SMOKE_OUT={})", out.display());
    let Ok(mut child) = Command::new(exe).env("SUPERAPP_SMOKE_OUT", &out).spawn() else {
        eprintln!("nÃ£o consegui iniciar {exe}");
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            eprintln!("smoke: timeout de 90 s sem relatÃ³rio");
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let Ok(text) = std::fs::read_to_string(&out) else {
        eprintln!("smoke: o app saiu sem gravar o relatÃ³rio");
        return false;
    };
    let _ = std::fs::remove_file(&out);
    let Ok(report) = serde_json::from_str::<Value>(&text) else {
        eprintln!("smoke: relatÃ³rio invÃ¡lido: {text}");
        return false;
    };
    check_report(&report)
}

fn f_unknown(f: &Value) -> &Value {
    &f["unknown"]
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
            "args invÃ¡lidos â†’ invalid_args",
            code(&r["echo_invalid"]) == "invalid_args",
        ),
        (
            "comando inexistente â†’ not_found",
            code(&r["unknown"]) == "not_found",
        ),
        (
            "sem capability â†’ permission_denied",
            code(&r["privileged"]) == "permission_denied",
        ),
        (
            "hook: nome invÃ¡lido barrado",
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
            "sapp: round-trip 1 MiB e 10 MiB Ã­ntegros",
            f["echo_1mib"] == true && f["gen_ok"] == true && f["echo10_ok"] == true,
        ),
        (
            "sapp: desconhecido â†’ not_found",
            code(f_unknown(f)) == "not_found",
        ),
        (
            "sapp: entrada > 16 MiB â†’ invalid_args",
            code(&f["over_in"]) == "invalid_args",
        ),
        (
            "sapp: saÃ­da > 16 MiB â†’ internal",
            code(&f["over_out"]) == "internal",
        ),
        (
            "sapp: argumento malformado â†’ invalid_args",
            code(&f["bad_gen"]) == "invalid_args",
        ),
        (
            "sapp: untrusted negado",
            code(&r["untrusted"]["blob"]) == "permission_denied",
        ),
    ]);
    checks.extend([
        (
            "bytes: round-trip 4 B e 1 MiB Ã­ntegros",
            b["echo_small"] == true && b["echo_1mib"] == true,
        ),
        ("bytes: 10 MiB gerados Ã­ntegros", b["gen_ok"] == true),
        (
            "bytes: comando desconhecido â†’ not_found",
            code(&b["unknown"]) == "not_found",
        ),
        (
            "bytes: entrada > 1 MiB barrada pelo hook",
            code(&b["over_in"]) == "permission_denied",
        ),
        (
            "bytes: saÃ­da > 16 MiB â†’ internal",
            code(&b["over_out"]) == "internal",
        ),
        (
            "bytes: argumento malformado â†’ invalid_args",
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
            "auditoria: rejeiÃ§Ã£o do hook (plugin)",
            has("main", "plugin:fs|read_text_file", "DeniedByHook"),
        ),
        (
            "auditoria: rejeiÃ§Ã£o do hook (nome invÃ¡lido, sanitizado)",
            has("main", "Bad?Name?", "DeniedByHook"),
        ),
    ]);

    let mut ok = true;
    for (name, pass) in &checks {
        println!("  {} {name}", if *pass { "ok  " } else { "FALHA" });
        ok &= *pass;
    }
    if !ok {
        eprintln!("relatÃ³rio completo: {report}");
    }
    ok
}

/// Benchmark de release: tamanho do binÃ¡rio, tempo atÃ© a UI carregar e memÃ³ria (host + WebView).
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
        eprintln!("nÃ£o consegui iniciar {exe}");
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
    // Deixa a UI assentar e soma a memÃ³ria do app e de todos os descendentes (WebView).
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

| mÃ©trica | valor |
|---|---|
| binÃ¡rio release | {} MB |
| UI pronta | {} ms |
| RAM host | {} MB |
| RAM total (host + WebView, {} processos) | {} MB |
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
    // OrÃ§amento frouxo de regressÃ£o (revisar com dados reais): UI pronta em < 15 s.
    startup_ms > 0 && startup_ms < 15_000
}
