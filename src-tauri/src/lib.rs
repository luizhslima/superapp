//! Host Tauri: encaminha todo comando da UI para o `Router` do core.
use serde_json::{json, Value};
use std::sync::Arc;
use superapp_core::{
    AuditEvent, AuditSink, Caller, ErrorCode, IpcError, MemoryAudit, Policy, Router, TracingAudit,
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
    router.dispatch(
        &Caller::new(window.label()),
        &command,
        args.unwrap_or(Value::Null),
    )
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

fn build_router(smoke_out: Option<String>, app: tauri::AppHandle) -> Router {
    let memory = Arc::new(MemoryAudit::default());
    let audit: Arc<dyn AuditSink> = Arc::new(Fanout(vec![
        Arc::new(TracingAudit),
        memory.clone() as Arc<dyn AuditSink>,
    ]));

    // Zero privilégio: cada janela recebe só o que precisa. "untrusted" nunca recebe nada.
    let mut policy = Policy::new()
        .grant("main", "app:read")
        .grant("untrusted", "none");
    if smoke_out.is_some() {
        policy = policy.grant("main", "smoke");
    }

    let mut r = Router::new(policy, audit);
    let smoke = smoke_out.is_some();
    r.register("app.info", "app:read", move |_c, _a: Value| {
        Ok(json!({ "version": superapp_core::VERSION, "smoke": smoke }))
    });
    r.register(
        "app.echo",
        "app:read",
        |_c, a: serde_json::Map<String, Value>| match a.get("text").and_then(Value::as_str) {
            Some(t) if t.len() <= 1024 => Ok(json!({ "text": t })),
            _ => Err(IpcError::new(ErrorCode::InvalidArgs, "text inválido")),
        },
    );
    // Existe, mas nenhuma janela tem a capability: serve para provar o bloqueio.
    r.register("debug.privileged", "debug:privileged", |_c, _a: Value| {
        Ok(json!("segredo"))
    });

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
    tracing_subscriber::fmt().with_target(false).init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ipc])
        .setup(|app| {
            let smoke_out = std::env::var("SUPERAPP_SMOKE_OUT").ok();
            app.manage(build_router(smoke_out.clone(), app.handle().clone()));
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
