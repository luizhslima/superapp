//! Trilha de auditoria: toda decisão do roteador (permitida ou negada) é registrada.
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    DeniedUnknownCommand,
    DeniedNoCapability,
    /// Mensagem barrada pelo hook de isolamento antes de chegar ao roteador.
    DeniedByHook,
    /// Corpo binário acima do limite do comando (entrada) ou resposta acima do limite (saída).
    DeniedTooLarge,
    /// Chamada de rede de um mini-app barrada pelas regras do host (domínio, IP, limite...).
    DeniedByNetworkRule,
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
        let mut v = self.events.lock().expect("audit mutex");
        // Memória limitada: ao passar do teto, descarta os mais antigos (10% de uma vez).
        if v.len() >= MEMORY_MAX_EVENTS {
            v.drain(..MEMORY_MAX_EVENTS / 10);
        }
        v.push(event);
    }
}

/// Teto de eventos guardados em memória por `MemoryAudit`.
pub const MEMORY_MAX_EVENTS: usize = 100_000;

/// Tamanho máximo de cada arquivo de auditoria antes de rotacionar (5 MiB).
pub const DEFAULT_MAX_BYTES: u64 = 5 * 1024 * 1024;
/// Quantos arquivos antigos manter (`audit.jsonl.1` ... `.N`); o mais antigo é descartado.
pub const DEFAULT_KEEP: usize = 3;

struct FileState {
    file: std::fs::File,
    size: u64,
}

/// Sink persistente: uma linha JSON por evento (JSONL), em modo append, com **rotação por
/// tamanho**: um cliente barulhento (ou hostil) não consegue encher o disco. Ao passar do
/// limite, `audit.jsonl` vira `.1`, `.1` vira `.2` etc., e o mais antigo é apagado.
/// Falha de escrita nunca derruba o app: vira `tracing::error`.
pub struct FileAudit {
    path: std::path::PathBuf,
    max_bytes: u64,
    keep: usize,
    state: Mutex<FileState>,
}

impl std::fmt::Debug for FileAudit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileAudit")
            .field("path", &self.path)
            .finish()
    }
}

fn open_append(path: &std::path::Path) -> std::io::Result<FileState> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    Ok(FileState { file, size })
}

impl FileAudit {
    pub fn open(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        Self::open_with(path, DEFAULT_MAX_BYTES, DEFAULT_KEEP)
    }

    pub fn open_with(
        path: impl AsRef<std::path::Path>,
        max_bytes: u64,
        keep: usize,
    ) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let state = open_append(&path)?;
        Ok(Self {
            path,
            max_bytes: max_bytes.max(1),
            keep: keep.max(1),
            state: Mutex::new(state),
        })
    }

    fn rotated(&self, n: usize) -> std::path::PathBuf {
        let mut name = self.path.as_os_str().to_owned();
        name.push(format!(".{n}"));
        std::path::PathBuf::from(name)
    }

    fn rotate(&self, st: &mut FileState) -> std::io::Result<()> {
        let _ = std::fs::remove_file(self.rotated(self.keep));
        for n in (1..self.keep).rev() {
            let _ = std::fs::rename(self.rotated(n), self.rotated(n + 1));
        }
        std::fs::rename(&self.path, self.rotated(1))?;
        *st = open_append(&self.path)?;
        Ok(())
    }
}

impl AuditSink for FileAudit {
    fn record(&self, e: AuditEvent) {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let line = format!(
            "{}
",
            serde_json::json!({
                "ts_ms": ts,
                "window": e.window,
                "command": e.command,
                "decision": format!("{:?}", e.decision),
            })
        );
        let mut st = self.state.lock().expect("audit file mutex");
        if st.size > 0 && st.size + line.len() as u64 > self.max_bytes {
            if let Err(err) = self.rotate(&mut st) {
                tracing::error!(error = %err, "falha ao rotacionar a auditoria");
            }
        }
        match st.file.write_all(line.as_bytes()) {
            Ok(()) => st.size += line.len() as u64,
            Err(err) => tracing::error!(error = %err, "falha ao gravar auditoria"),
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

#[cfg(test)]
mod memory_tests {
    use super::*;

    #[test]
    fn memory_audit_is_bounded_and_keeps_the_newest() {
        let a = MemoryAudit::default();
        for i in 0..(MEMORY_MAX_EVENTS + 5_000) {
            a.record(AuditEvent {
                window: "w".into(),
                command: format!("c{i}"),
                decision: Decision::Allowed,
            });
        }
        let ev = a.events();
        assert!(ev.len() <= MEMORY_MAX_EVENTS);
        assert_eq!(
            ev.last().unwrap().command,
            format!("c{}", MEMORY_MAX_EVENTS + 4_999)
        );
    }
}
