//! Manifesto do mini-app: o que ele é, o que pede e o hash do seu código.
use crate::PackageError;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

/// Tetos que o host aceita num manifesto; pedir mais invalida o pacote.
pub const MAX_FUEL: u64 = 500_000_000;
pub const MAX_MEMORY_MB: u32 = 64;
pub const MAX_TIMEOUT_MS: u32 = 30_000;

/// Permissão pedida pelo mini-app (uma por interface WIT do host).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Log,
    Storage,
    Clock,
}

/// Limites pedidos (opcionais). O host usa o menor entre o pedido e o seu teto.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedLimits {
    pub fuel: Option<u64>,
    pub memory_mb: Option<u32>,
    pub timeout_ms: Option<u32>,
}

/// `deny_unknown_fields`: um campo que esta versão do host não entende invalida o pacote,
/// em vez de ser ignorado em silêncio (ignorar permissões desconhecidas seria perigoso).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub permissions: Vec<Permission>,
    /// SHA-256 do `.wasm`, em hexadecimal minúsculo (64 caracteres). Pode ficar de fora do
    /// modelo de manifesto escrito à mão: `pack` calcula e preenche.
    #[serde(default)]
    pub wasm_sha256: String,
    #[serde(default)]
    pub limits: Option<RequestedLimits>,
}

/// Id aceito como nome de diretório e identidade: `[a-z0-9-]{1,32}`.
pub fn valid_app_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn invalid(msg: &str) -> PackageError {
    PackageError::InvalidManifest(msg.to_owned())
}

impl Manifest {
    pub fn validate(&self) -> Result<(), PackageError> {
        if self.schema != SCHEMA_VERSION {
            return Err(invalid("schema não suportado"));
        }
        if !valid_app_id(&self.id) {
            return Err(invalid("id inválido (use [a-z0-9-], até 32)"));
        }
        let n = self.name.chars().count();
        if n == 0 || n > 64 || self.name.chars().any(char::is_control) {
            return Err(invalid("nome inválido (1 a 64 caracteres, sem controle)"));
        }
        let parts: Vec<&str> = self.version.split('.').collect();
        let version_ok = parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()));
        if !version_ok {
            return Err(invalid("versão deve ser N.N.N"));
        }
        let mut seen = self.permissions.clone();
        seen.sort();
        seen.dedup();
        if seen.len() != self.permissions.len() {
            return Err(invalid("permissão repetida"));
        }
        if self.wasm_sha256.len() != 64
            || !self
                .wasm_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid("wasm_sha256 deve ter 64 hex minúsculos"));
        }
        if let Some(l) = &self.limits {
            if l.fuel.is_some_and(|v| v == 0 || v > MAX_FUEL) {
                return Err(invalid("limits.fuel fora do teto do host"));
            }
            if l.memory_mb.is_some_and(|v| v == 0 || v > MAX_MEMORY_MB) {
                return Err(invalid("limits.memory_mb fora do teto do host"));
            }
            if l.timeout_ms.is_some_and(|v| v == 0 || v > MAX_TIMEOUT_MS) {
                return Err(invalid("limits.timeout_ms fora do teto do host"));
            }
        }
        Ok(())
    }
}
