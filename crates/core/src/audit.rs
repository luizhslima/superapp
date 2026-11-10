//! Trilha de auditoria: toda decisão do roteador (permitida ou negada) é registrada.
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    DeniedUnknownCommand,
    DeniedNoCapability,
    /// Mensagem barrada pelo hook de isolamento antes de chegar ao roteador.
    DeniedByHook,
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

/// Sink persistente: uma linha JSON por evento (JSONL), em modo append.
/// Falha de escrita nunca derruba o app: vira `tracing::error`.
#[derive(Debug)]
pub struct FileAudit {
    file: Mutex<std::fs::File>,
}

impl FileAudit {
    pub fn open(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl AuditSink for FileAudit {
    fn record(&self, e: AuditEvent) {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let line = serde_json::json!({
            "ts_ms": ts,
            "window": e.window,
            "command": e.command,
            "decision": format!("{:?}", e.decision),
        });
        let mut f = self.file.lock().expect("audit file mutex");
        if let Err(err) = writeln!(f, "{line}") {
            tracing::error!(error = %err, "falha ao gravar auditoria");
        }
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
