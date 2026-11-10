//! Host Tauri: encaminha todo comando da UI para o `Router` do core.
use serde_json::{json, Value};
use std::sync::Arc;
use superapp_core::{
    AuditEvent, AuditSink, Caller, ErrorCode, FileAudit, IpcError, MemoryAudit, Policy, Router,
    TracingAudit,
};
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

/// Único comando exposto à UI. A identidade (`window.label()`) vem do Tauri, não do payload:
/// a UI não consegue se passar por outra janela.
#[tauri::command]
fn ipc(
    window: tauri::WebviewWindow,
    router: State<'_, Router>,
    command: String,
    args: Option<Value>,
) -> Result<Value, IpcError> {
    let caller = Caller::new(window.label());
    if command == HOOK_REJECTED {
        // Só o hook de isolamento produz este nome (a UI não passa no filtro de formato).
        let attempted = args
            .as_ref()
            .and_then(|a| a.get("attempted"))
            .and_then(Value::as_str)
            .unwrap_or("?");
        router.audit_hook_rejection(&caller, attempted);
        return Err(IpcError::new(ErrorCode::PermissionDenied, "não autorizado"));
    }
    router.dispatch(&caller, &command, args.unwrap_or(Value::Null))
}

/// Comando reservado: o hook de isolamento reescreve nele toda mensagem que barra.
const HOOK_REJECTED: &str = "HOOK_REJECTED";

/// Limite de corpo binário (entrada e saída) dos comandos `blob.*`: 16 MiB.
const BLOB_MAX: usize = 16 * 1024 * 1024;

/// Canal binário: corpo cru (sem JSON/base64) e nome do comando no cabeçalho `command`.
/// Mesma política do `ipc`: identidade = label da janela, decisão no Router.
#[tauri::command]
fn ipc_bytes(
    window: tauri::WebviewWindow,
    router: State<'_, Router>,
    request: tauri::ipc::Request<'_>,
) -> Result<tauri::ipc::Response, IpcError> {
    let caller = Caller::new(window.label());
    let command = request
        .headers()
        .get("command")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let tauri::ipc::InvokeBody::Raw(body) = request.body() else {
        return Err(IpcError::new(
            ErrorCode::InvalidArgs,
            "corpo deve ser binário",
        ));
    };
    router
        .dispatch_bytes(&caller, command, body)
        .map(tauri::ipc::Response::new)
}

/// Canal de corpos grandes por esquema de URI próprio (`sapp://localhost/<comando>`, POST).
/// O Isolation Pattern cifra/serializa cada byte (~1 MiB/s medido), então blobs não passam
/// por ele. A identidade continua vindo do Tauri (`webview_label`), a decisão continua no
/// Router. Limite: o Tauri entrega o corpo inteiro em memória antes do handler.
fn blob_protocol(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::{Method, Response, StatusCode};
    let cors = |b: tauri::http::response::Builder| {
        b.header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Methods", "POST, OPTIONS")
    };
    if request.method() == Method::OPTIONS {
        return cors(Response::builder())
            .status(StatusCode::NO_CONTENT)
            .body(Vec::new())
            .expect("resposta");
    }
    let router = ctx.app_handle().state::<Router>();
    let caller = Caller::new(ctx.webview_label());
    let command = request.uri().path().trim_start_matches('/');
    match router.dispatch_bytes(&caller, command, request.body()) {
        Ok(out) => cors(Response::builder())
            .status(StatusCode::OK)
            .body(out)
            .expect("resposta"),
        Err(e) => {
            let status = match e.code {
                ErrorCode::NotFound => StatusCode::NOT_FOUND,
                ErrorCode::PermissionDenied => StatusCode::FORBIDDEN,
                ErrorCode::InvalidArgs => StatusCode::BAD_REQUEST,
                ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            };
            cors(Response::builder())
                .status(status)
                .header("Content-Type", "application/json")
                .body(serde_json::to_vec(&e).unwrap_or_default())
                .expect("resposta")
        }
    }
}

/// Sink que replica para o log estruturado e guarda em memória (usado só no modo smoke).
struct Fanout(Vec<Arc<dyn AuditSink>>);
impl AuditSink for Fanout {
    fn record(&self, e: AuditEvent) {
        for s in &self.0 {
            s.record(e.clone());
        }
    }
}

fn build_router(
    smoke_out: Option<String>,
    bench_out: Option<String>,
    start: std::time::Instant,
    app: tauri::AppHandle,
) -> Router {
    let memory = Arc::new(MemoryAudit::default());
    let mut sinks: Vec<Arc<dyn AuditSink>> =
        vec![Arc::new(TracingAudit), memory.clone() as Arc<dyn AuditSink>];
    // Trilha persistente em <log_dir>/audit.jsonl (JSONL, append).
    match app.path().app_log_dir() {
        Ok(dir) => match std::fs::create_dir_all(&dir)
            .and_then(|_| FileAudit::open(dir.join("audit.jsonl")))
        {
            Ok(f) => sinks.push(Arc::new(f)),
            Err(e) => tracing::error!(error = %e, "auditoria em arquivo desativada"),
        },
        Err(e) => tracing::error!(error = %e, "sem diretório de log"),
    }
    let audit: Arc<dyn AuditSink> = Arc::new(Fanout(sinks));

    // Zero privilégio: cada janela recebe só o que precisa. "untrusted" nunca recebe nada.
    let mut policy = Policy::new()
        .grant("main", "app:read")
        .grant("untrusted", "none")
        .grant("main", "shell:manage")
        .grant("main", "blob:use")
        .grant("mfe-a", "app:read")
        .grant("mfe-b", "app:read");
    if bench_out.is_some() {
        policy = policy.grant("main", "bench");
    }
    if smoke_out.is_some() {
        policy = policy.grant("main", "smoke");
    }

    let mut r = Router::new(policy, audit);
    let smoke = smoke_out.is_some();
    let bench = bench_out.is_some();
    r.register("app.info", "app:read", move |_c, _a: Value| {
        Ok(json!({ "version": superapp_core::VERSION, "smoke": smoke, "bench": bench }))
    });
    r.register(
        "app.echo",
        "app:read",
        |_c, a: serde_json::Map<String, Value>| match a.get("text").and_then(Value::as_str) {
            Some(t) if t.len() <= 1024 => Ok(json!({ "text": t })),
            _ => Err(IpcError::new(ErrorCode::InvalidArgs, "text inválido")),
        },
    );
    // Abre um micro-frontend em janela própria. Só ids da lista fixa (nada vindo da UI vira label).
    let handle = app.clone();
    r.register(
        "shell.open_mfe",
        "shell:manage",
        move |_c, a: serde_json::Map<String, Value>| {
            let id = match a.get("id").and_then(Value::as_str) {
                Some(i @ ("a" | "b")) => i.to_owned(),
                _ => return Err(IpcError::new(ErrorCode::InvalidArgs, "id inválido")),
            };
            let h = handle.clone();
            // Criar janela dentro de comando síncrono trava no Windows: faz em outra thread.
            std::thread::spawn(move || {
                let res = WebviewWindowBuilder::new(
                    &h,
                    format!("mfe-{id}"),
                    WebviewUrl::App(format!("mfe-{id}.html").into()),
                )
                .title(format!("mfe {id}"))
                .build();
                if let Err(e) = res {
                    tracing::error!(error = %e, "falha ao abrir micro-frontend");
                }
            });
            Ok(Value::Null)
        },
    );
    // Binários: eco e gerador (4 bytes LE = tamanho pedido). Usados para validar e medir o canal.
    r.register_bytes("blob.echo", "blob:use", BLOB_MAX, BLOB_MAX, |_c, b| {
        Ok(b.to_vec())
    });
    r.register_bytes("blob.sink", "blob:use", BLOB_MAX, 4, |_c, b| {
        Ok((b.len() as u32).to_le_bytes().to_vec())
    });
    r.register_bytes("blob.gen", "blob:use", 4, BLOB_MAX, |_c, b| {
        let n: [u8; 4] = b
            .try_into()
            .map_err(|_| IpcError::new(ErrorCode::InvalidArgs, "esperado u32 LE"))?;
        Ok(vec![0xAB; u32::from_le_bytes(n) as usize])
    });
    // Existe, mas nenhuma janela tem a capability: serve para provar o bloqueio.
    r.register("debug.privileged", "debug:privileged", |_c, _a: Value| {
        Ok(json!("segredo"))
    });

    // Modo benchmark: a UI avisa que carregou; grava o tempo desde o início do processo.
    // O app continua vivo para o xtask medir memória, e é encerrado por ele.
    if let Some(path) = bench_out {
        r.register("bench.ready", "bench", move |_c, _a: Value| {
            let ms = start.elapsed().as_millis();
            let _ = std::fs::write(&path, json!({ "startup_ms": ms }).to_string());
            Ok(Value::Null)
        });
    }
    if let Some(path) = smoke_out {
        r.register("smoke.report", "smoke", move |_c, a: Value| {
            let events: Vec<Value> = memory
                .events()
                .iter()
                .map(|e| json!({ "window": e.window, "command": e.command, "decision": format!("{:?}", e.decision) }))
                .collect();
            let report = json!({ "results": a, "audit": events });
            let _ = std::fs::write(&path, report.to_string());
            app.exit(0);
            Ok(Value::Null)
        });
    }
    r
}

pub fn run() {
    let start = std::time::Instant::now();
    tracing_subscriber::fmt().with_target(false).init();

    tauri::Builder::default()
        .register_uri_scheme_protocol("sapp", blob_protocol)
        .invoke_handler(tauri::generate_handler![ipc, ipc_bytes])
        .setup(move |app| {
            let smoke_out = std::env::var("SUPERAPP_SMOKE_OUT").ok();
            let bench_out = std::env::var("SUPERAPP_BENCH_OUT").ok();
            app.manage(build_router(
                smoke_out.clone(),
                bench_out,
                start,
                app.handle().clone(),
            ));
            if smoke_out.is_some() {
                // Janela sem nenhuma capability, só no modo smoke.
                WebviewWindowBuilder::new(
                    app,
                    "untrusted",
                    WebviewUrl::App("smoke-untrusted.html".into()),
                )
                .title("untrusted")
                .build()?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao rodar o app");
}
