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
    /// Rede mediada pelo host (ADR-011); exige `network.hosts` no manifesto.
    Http,
}

/// Declaração de rede: domínios que o mini-app pode alcançar (só `https`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub hosts: Vec<String>,
}

/// Máximo de entradas em `network.hosts`.
pub const MAX_NETWORK_HOSTS: usize = 8;

/// Padrão de domínio aceito: nome DNS em minúsculas com ao menos um ponto, ou `*.` + nome
/// (um nível). Nunca IP literal, nunca nome de rótulo único (`localhost`).
pub fn valid_network_host(h: &str) -> bool {
    let name = h.strip_prefix("*.").unwrap_or(h);
    if h.len() > 253 || name.is_empty() || name.parse::<std::net::IpAddr>().is_ok() {
        return false;
    }
    let labels: Vec<&str> = name.split('.').collect();
    // O último rótulo precisa ter letra: bloqueia formas numéricas como `127.1`.
    labels.len() >= 2
        && labels
            .last()
            .is_some_and(|l| l.bytes().any(|b| b.is_ascii_lowercase()))
        && labels.iter().all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
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
    /// Domínios alcançáveis; obrigatório (e só aceito) com a permissão `http`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<Network>,
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
        let has_http = self.permissions.contains(&Permission::Http);
        match (&self.network, has_http) {
            (Some(n), true) => {
                if n.hosts.is_empty() || n.hosts.len() > MAX_NETWORK_HOSTS {
                    return Err(invalid("network.hosts: de 1 a 8 domínios"));
                }
                if !n.hosts.iter().all(|h| valid_network_host(h)) {
                    return Err(invalid(
                        "network.hosts: domínio inválido (sem IP, só https)",
                    ));
                }
                let mut h = n.hosts.clone();
                h.sort();
                h.dedup();
                if h.len() != n.hosts.len() {
                    return Err(invalid("network.hosts: domínio repetido"));
                }
            }
            (None, true) => return Err(invalid("a permissão http exige network.hosts")),
            (Some(_), false) => return Err(invalid("network só vale com a permissão http")),
            (None, false) => {}
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

#[cfg(test)]
mod network_tests {
    use super::*;

    fn base() -> Manifest {
        Manifest {
            schema: 1,
            id: "web".into(),
            name: "Web".into(),
            version: "0.1.0".into(),
            permissions: vec![Permission::Http],
            wasm_sha256: "0".repeat(64),
            limits: None,
            network: Some(Network {
                hosts: vec!["api.exemplo.com".into(), "*.reddit.com".into()],
            }),
        }
    }

    #[test]
    fn http_needs_hosts_and_hosts_need_http() {
        assert!(base().validate().is_ok());
        let mut m = base();
        m.network = None;
        assert!(m.validate().is_err(), "http sem network");
        let mut m = base();
        m.permissions = vec![Permission::Storage];
        assert!(m.validate().is_err(), "network sem http");
        let mut m = base();
        m.network = Some(Network { hosts: vec![] });
        assert!(m.validate().is_err(), "lista vazia");
    }

    #[test]
    fn host_patterns_are_strict() {
        for ok in [
            "a.com",
            "api.exemplo.com",
            "*.reddit.com",
            "xn--e1afmkfd.xn--p1ai",
        ] {
            assert!(valid_network_host(ok), "{ok}");
        }
        for bad in [
            "",
            "localhost",
            "*",
            "*.com.*",
            "a.*.com",
            "**.a.com",
            "A.com",
            "a_b.com",
            "-a.com",
            "a-.com",
            "a..com",
            ".a.com",
            "a.com.",
            "127.0.0.1",
            "127.1",
            "1.2.3.4",
            "[::1]",
            "::1",
            "*.127.0.0.1",
            "a.com:443",
            "a.com/x",
            "user@a.com",
            "http://a.com",
            "a com",
            "a.c\u{0}om",
        ] {
            assert!(!valid_network_host(bad), "{bad:?}");
        }
        assert!(!valid_network_host(&format!("{}.com", "a".repeat(64))));
        assert!(!valid_network_host(&format!("{}.com", "a.".repeat(130))));
    }

    #[test]
    fn host_count_and_duplicates_are_limited() {
        let mut m = base();
        m.network = Some(Network {
            hosts: (0..=MAX_NETWORK_HOSTS)
                .map(|i| format!("h{i}.com"))
                .collect(),
        });
        assert!(m.validate().is_err());
        m.network = Some(Network {
            hosts: vec!["a.com".into(), "a.com".into()],
        });
        assert!(m.validate().is_err());
    }

    #[test]
    fn unknown_network_fields_are_rejected_and_old_manifests_do_not_change() {
        let j = r#"{"schema":1,"id":"a","name":"A","version":"0.1.0","permissions":["http"],"network":{"hosts":["a.com"],"extra":1}}"#;
        assert!(serde_json::from_str::<Manifest>(j).is_err());
        // Sem rede, o campo nem é escrito: o JSON (e a assinatura) dos pacotes antigos não muda.
        let mut m = base();
        m.permissions = vec![Permission::Storage];
        m.network = None;
        assert!(!serde_json::to_string(&m).unwrap().contains("network"));
    }
}
