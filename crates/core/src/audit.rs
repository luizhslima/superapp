//! Trilha de auditoria: toda decisão do roteador (permitida ou negada) é registrada.
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    DeniedUnknownCommand,
    DeniedNoCapability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub window: String,
    pub command: String,
    pub decision: Decision,
}

/// Destino dos eventos. Em produção: arquivo/tracing; em testes: memória.
pub trait AuditSink: Send + Sync {
    fn record(&self, event: AuditEvent);
}

/// Sink em memória, útil em testes e para uma tela de diagnóstico.
#[derive(Debug, Default)]
pub struct MemoryAudit {
    events: Mutex<Vec<AuditEvent>>,
}

impl MemoryAudit {
    pub fn events(&self) -> Vec<AuditEvent> {
        self.events.lock().expect("audit mutex").clone()
    }
}

impl AuditSink for MemoryAudit {
    fn record(&self, event: AuditEvent) {
        self.events.lock().expect("audit mutex").push(event);
    }
}

/// Sink que escreve via `tracing` (log estruturado). Negações saem como `warn`.
#[derive(Debug, Default)]
pub struct TracingAudit;

impl AuditSink for TracingAudit {
    fn record(&self, e: AuditEvent) {
        match e.decision {
            Decision::Allowed => {
                tracing::info!(window = %e.window, command = %e.command, "ipc permitido")
            }
            d => {
                tracing::warn!(window = %e.window, command = %e.command, decision = ?d, "ipc negado")
            }
        }
    }
}
