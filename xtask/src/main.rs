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
        _ => {
            eprintln!("uso: cargo xtask <fmt|lint|test|ci|smoke>");
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
    println!("> {exe} (SUPERAPP_SMOKE_OUT={})", out.display());
    let Ok(mut child) = Command::new(exe).env("SUPERAPP_SMOKE_OUT", &out).spawn() else {
        eprintln!("não consegui iniciar {exe}");
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(90);
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
