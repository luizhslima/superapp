//! Roteador IPC: registro de comandos tipados + checagem de capability + auditoria.
use crate::audit::{AuditEvent, AuditSink, Decision};
use crate::error::{ErrorCode, IpcError};
use crate::policy::{Caller, Policy};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

type Handler = Box<dyn Fn(&Caller, Value) -> Result<Value, IpcError> + Send + Sync>;

struct Command {
    capability: String,
    handler: Handler,
}

pub struct Router {
    commands: HashMap<String, Command>,
    policy: Policy,
    audit: Arc<dyn AuditSink>,
}

impl Router {
    pub fn new(policy: Policy, audit: Arc<dyn AuditSink>) -> Self {
        Self {
            commands: HashMap::new(),
            policy,
            audit,
        }
    }

    /// Registra um comando tipado. Argumentos e retorno passam por serde; um JSON
    /// que não bate com `A` vira `InvalidArgs` antes de o handler ser chamado.
    pub fn register<A, R, F>(&mut self, name: &str, capability: &str, f: F)
    where
        A: DeserializeOwned,
        R: Serialize,
        F: Fn(&Caller, A) -> Result<R, IpcError> + Send + Sync + 'static,
    {
        let handler: Handler = Box::new(move |caller, raw| {
            let args: A = serde_json::from_value(raw)
                .map_err(|e| IpcError::new(ErrorCode::InvalidArgs, e.to_string()))?;
            let out = f(caller, args)?;
            serde_json::to_value(out).map_err(|e| {
                tracing::error!(error = %e, "falha ao serializar resposta");
                IpcError::internal()
            })
        });
        self.commands.insert(
            name.to_owned(),
            Command {
                capability: capability.to_owned(),
                handler,
            },
        );
    }

    /// Ponto único de entrada. Ordem: existe? → autorizado? → valida args → executa.
    pub fn dispatch(&self, caller: &Caller, name: &str, args: Value) -> Result<Value, IpcError> {
        let Some(cmd) = self.commands.get(name) else {
            self.record(caller, name, Decision::DeniedUnknownCommand);
            return Err(IpcError::new(ErrorCode::NotFound, "comando desconhecido"));
        };
        if !self.policy.allows(caller, &cmd.capability) {
            self.record(caller, name, Decision::DeniedNoCapability);
            // Mensagem genérica: não revela qual capability faltou.
            return Err(IpcError::new(ErrorCode::PermissionDenied, "não autorizado"));
        }
        self.record(caller, name, Decision::Allowed);
        (cmd.handler)(caller, args)
    }

    /// Registra uma mensagem que o hook de isolamento barrou. O nome tentado vem da UI
    /// (não confiável): é truncado e sanitizado para não poluir nem forjar o log.
    pub fn audit_hook_rejection(&self, caller: &Caller, attempted: &str) {
        let safe: String = attempted
            .chars()
            .take(64)
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '|' | '-') {
                    c
                } else {
                    '?'
                }
            })
            .collect();
        self.record(caller, &safe, Decision::DeniedByHook);
    }

    fn record(&self, caller: &Caller, command: &str, decision: Decision) {
        self.audit.record(AuditEvent {
            window: caller.window.clone(),
            command: command.to_owned(),
            decision,
        });
    }
}
