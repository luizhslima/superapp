//! Erros padronizados do IPC. Serializáveis para devolver à UI sem vazar detalhes internos.
use serde::{Deserialize, Serialize};

/// Código estável e legível por máquina; a UI decide o que fazer com ele.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    PermissionDenied,
    InvalidArgs,
    Internal,
}

/// Erro devolvido pelo roteador (e por handlers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct IpcError {
    pub code: ErrorCode,
    pub message: String,
}

impl IpcError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Erro interno genérico: o detalhe real vai para o log, não para a UI.
    pub fn internal() -> Self {
        Self::new(ErrorCode::Internal, "erro interno")
    }
}
