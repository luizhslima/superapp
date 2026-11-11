//! Núcleo do host: roteador IPC, capabilities e auditoria.
//!
//! Sem dependência de Tauri de propósito: a lógica de segurança é testável isoladamente
//! e o app Tauri apenas encaminha cada comando para [`Router::dispatch`].

pub mod audit;
pub mod error;
pub mod policy;
pub mod router;

pub use audit::{AuditEvent, AuditSink, Decision, FileAudit, MemoryAudit, TracingAudit};
pub use error::{ErrorCode, IpcError};
pub use policy::{Caller, Policy};
pub use router::Router;

/// Versão do núcleo, usada em handshakes e diagnósticos.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::sync::Arc;

    #[derive(Deserialize)]
    struct AddArgs {
        a: i32,
        b: i32,
    }

    #[derive(Serialize)]
    struct Sum {
        sum: i32,
    }

    fn setup() -> (Router, Arc<MemoryAudit>) {
        let audit = Arc::new(MemoryAudit::default());
        let policy = Policy::new().grant("main", "math");
        let mut r = Router::new(policy, audit.clone());
        r.register("add", "math", |_c, a: AddArgs| Ok(Sum { sum: a.a + a.b }));
        r.register(
            "fail",
            "math",
            |_c, _a: serde_json::Value| -> Result<(), _> {
                Err(IpcError::new(ErrorCode::Internal, "boom"))
            },
        );
        (r, audit)
    }

    #[test]
    fn allowed_command_runs_and_is_audited() {
        let (r, audit) = setup();
        let out = r
            .dispatch(&Caller::new("main"), "add", json!({"a": 2, "b": 3}))
            .unwrap();
        assert_eq!(out, json!({"sum": 5}));
        assert_eq!(audit.events()[0].decision, Decision::Allowed);
    }

    #[test]
    fn window_without_capability_is_blocked_and_logged() {
        let (r, audit) = setup();
        let err = r
            .dispatch(&Caller::new("other"), "add", json!({"a": 1, "b": 1}))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        let ev = &audit.events()[0];
        assert_eq!(ev.decision, Decision::DeniedNoCapability);
        assert_eq!(ev.window, "other");
        assert_eq!(ev.command, "add");
    }

    #[test]
    fn denial_message_does_not_leak_capability_name() {
        let (r, _) = setup();
        let err = r
            .dispatch(&Caller::new("other"), "add", json!({}))
            .unwrap_err();
        assert!(!err.message.contains("math"));
    }

    #[test]
    fn unknown_command_is_not_found_and_logged() {
        let (r, audit) = setup();
        let err = r
            .dispatch(&Caller::new("main"), "nope", json!(null))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(audit.events()[0].decision, Decision::DeniedUnknownCommand);
    }

    #[test]
    fn invalid_args_are_rejected_before_handler() {
        let (r, _) = setup();
        let err = r
            .dispatch(&Caller::new("main"), "add", json!({"a": "x"}))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgs);
    }

    #[test]
    fn denied_caller_cannot_probe_args_validation() {
        // Sem capability, nem chega a validar os argumentos (evita oráculo de schema).
        let (r, _) = setup();
        let err = r
            .dispatch(&Caller::new("other"), "add", json!({"a": "x"}))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::PermissionDenied);
    }

    #[test]
    fn handler_errors_pass_through() {
        let (r, _) = setup();
        let err = r
            .dispatch(&Caller::new("main"), "fail", json!(null))
            .unwrap_err();
        assert_eq!(err.message, "boom");
    }

    #[test]
    fn hook_rejection_is_audited_and_sanitized() {
        let (r, audit) = setup();
        let long = format!("evil\n\"{}", "x".repeat(200));
        r.audit_hook_rejection(&Caller::new("main"), &long);
        let ev = &audit.events()[0];
        assert_eq!(ev.decision, Decision::DeniedByHook);
        assert_eq!(ev.command.chars().count(), 64);
        assert!(ev.command.starts_with("evil??"));
    }

    #[test]
    fn file_audit_writes_one_json_line_per_event() {
        let path =
            std::env::temp_dir().join(format!("superapp-audit-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let sink = Arc::new(FileAudit::open(&path).unwrap());
        let mut r = Router::new(Policy::new().grant("main", "math"), sink);
        r.register("ping", "math", |_c, _a: serde_json::Value| Ok(1));
        r.dispatch(&Caller::new("main"), "ping", json!(null))
            .unwrap();
        let _ = r.dispatch(&Caller::new("other"), "ping", json!(null));
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["decision"], "Allowed");
        assert_eq!(lines[1]["decision"], "DeniedNoCapability");
        let _ = std::fs::remove_file(&path);
    }

    fn bytes_router() -> (Router, Arc<MemoryAudit>) {
        let audit = Arc::new(MemoryAudit::default());
        let mut r = Router::new(Policy::new().grant("main", "blob"), audit.clone());
        // Eco que dobra o tamanho: permite provocar estouro do limite de saída.
        r.register_bytes("blob.double", "blob", 16, 20, |_c, b| Ok([b, b].concat()));
        (r, audit)
    }

    #[test]
    fn bytes_roundtrip_and_audit() {
        let (r, audit) = bytes_router();
        let out = r
            .dispatch_bytes(&Caller::new("main"), "blob.double", &[1, 2])
            .unwrap();
        assert_eq!(out, vec![1, 2, 1, 2]);
        assert_eq!(audit.events()[0].decision, Decision::Allowed);
    }

    #[test]
    fn bytes_denied_without_capability_or_unknown() {
        let (r, audit) = bytes_router();
        let e = r
            .dispatch_bytes(&Caller::new("x"), "blob.double", &[1])
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::PermissionDenied);
        let e = r
            .dispatch_bytes(&Caller::new("main"), "nope", &[1])
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound);
        assert_eq!(audit.events()[0].decision, Decision::DeniedNoCapability);
        assert_eq!(audit.events()[1].decision, Decision::DeniedUnknownCommand);
    }

    #[test]
    fn bytes_input_over_limit_is_rejected_before_handler() {
        let (r, audit) = bytes_router();
        let e = r
            .dispatch_bytes(&Caller::new("main"), "blob.double", &[0; 17])
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidArgs);
        assert_eq!(audit.events()[0].decision, Decision::DeniedTooLarge);
    }

    #[test]
    fn bytes_output_over_limit_is_internal_error() {
        let (r, audit) = bytes_router();
        // 11 bytes → 22 > 20.
        let e = r
            .dispatch_bytes(&Caller::new("main"), "blob.double", &[0; 11])
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Internal);
        assert_eq!(
            audit.events().last().unwrap().decision,
            Decision::DeniedTooLarge
        );
    }

    #[test]
    fn hostile_names_never_reach_the_audit_log_raw() {
        // `sapp://` não passa pelo hook: o nome chega cru da UI. Quebras de linha e textos
        // gigantes não podem virar linhas forjadas no log nem no console.
        let (r, audit) = setup();
        let hostile = format!(
            "fake\n{{\"decision\":\"Allowed\"}}\r\x1b[31m{}",
            "x".repeat(500)
        );
        let _ = r.dispatch(&Caller::new("main"), &hostile, json!(null));
        let _ = r.dispatch_bytes(&Caller::new("ja\nnela"), &hostile, &[1]);
        let _ = r.dispatch(&Caller::new("other"), "add", json!({}));
        for ev in audit.events() {
            for field in [&ev.command, &ev.window] {
                assert!(field.chars().count() <= 64, "{field:?}");
                assert!(
                    !field
                        .chars()
                        .any(|c| c.is_control() || c == '"' || c == '{'),
                    "{field:?}"
                );
            }
        }
    }

    #[test]
    fn file_audit_rotates_by_size_and_keeps_only_recent_files() {
        let dir = std::env::temp_dir().join(format!("superapp-rot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("audit.jsonl");
        // Limite de 300 bytes e 2 arquivos antigos: com 60 eventos há várias rotações.
        let sink = Arc::new(FileAudit::open_with(&path, 300, 2).unwrap());
        let mut r = Router::new(Policy::new().grant("main", "math"), sink);
        r.register("ping", "math", |_c, _a: serde_json::Value| Ok(1));
        for _ in 0..60 {
            r.dispatch(&Caller::new("main"), "ping", json!(null))
                .unwrap();
        }
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(files, vec!["audit.jsonl", "audit.jsonl.1", "audit.jsonl.2"]);
        for f in &files {
            let len = std::fs::metadata(dir.join(f)).unwrap().len();
            assert!(len <= 300, "{f} tem {len} bytes (limite 300)");
        }
        // Os eventos mais recentes continuam no arquivo atual e as linhas estão inteiras.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.is_empty());
        for line in text.lines() {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ipc_error_serializes_with_stable_code() {
        let e = IpcError::new(ErrorCode::PermissionDenied, "x");
        assert_eq!(
            serde_json::to_value(e).unwrap(),
            json!({"code": "permission_denied", "message": "x"})
        );
    }
}
