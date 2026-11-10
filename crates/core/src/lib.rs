//! Núcleo do host: roteador IPC, capabilities e auditoria.
//!
//! Sem dependência de Tauri de propósito: a lógica de segurança é testável isoladamente
//! e o app Tauri apenas encaminha cada comando para [`Router::dispatch`].

pub mod audit;
pub mod error;
pub mod policy;
pub mod router;

pub use audit::{AuditEvent, AuditSink, Decision, MemoryAudit, TracingAudit};
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
    fn ipc_error_serializes_with_stable_code() {
        let e = IpcError::new(ErrorCode::PermissionDenied, "x");
        assert_eq!(
            serde_json::to_value(e).unwrap(),
            json!({"code": "permission_denied", "message": "x"})
        );
    }
}
