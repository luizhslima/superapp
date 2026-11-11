//! Catálogo estático assinado de mini-apps (E5.1): só a **verificação**, sem rede.
//!
//! Arquivo (JSON): `{ "pubkey": hex32, "sig": hex64, "payload": "<JSON em texto>" }`.
//! A assinatura Ed25519 cobre `DOMAIN || bytes exatos de payload`, então não há
//! canonicalização para errar. O signatário precisa estar no `TrustStore`; o catálogo tem
//! validade (`expires_at`), o que impede servir indefinidamente uma lista velha.
//! Cada entrada só aponta para um `.sapp` (HTTPS) e fixa o seu SHA-256: baixar e instalar
//! continua passando por `verify` do pacote e pelo consentimento do usuário.
use crate::{parse_hex32, sha256_hex, to_hex, valid_app_id, TrustStore};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

const DOMAIN: &[u8] = b"superapp-catalog-v1\0";
pub const MAX_CATALOG_BYTES: usize = 256 * 1024;
pub const MAX_ENTRIES: usize = 256;
const MAX_URL: usize = 512;
const MAX_TEXT: usize = 200;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CatalogError {
    #[error("catálogo grande demais")]
    TooLarge,
    #[error("catálogo malformado: {0}")]
    Malformed(String),
    #[error("assinante não confiável")]
    UntrustedSigner,
    #[error("assinatura inválida")]
    BadSignature,
    #[error("catálogo expirado")]
    Expired,
    #[error("entrada inválida: {0}")]
    BadEntry(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Sempre `https://`.
    pub url: String,
    /// SHA-256 (hex) do arquivo `.sapp` inteiro.
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub format: u32,
    /// Instante de emissão e de expiração (segundos Unix).
    pub issued_at: u64,
    pub expires_at: u64,
    pub entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    pubkey: String,
    sig: String,
    payload: String,
}

fn check_text(s: &str, what: &str, max: usize) -> Result<(), CatalogError> {
    if s.is_empty() || s.len() > max || s.chars().any(char::is_control) {
        return Err(CatalogError::BadEntry(format!("{what} inválido")));
    }
    Ok(())
}

impl Entry {
    fn validate(&self) -> Result<(), CatalogError> {
        if !valid_app_id(&self.id) {
            return Err(CatalogError::BadEntry("id inválido".into()));
        }
        check_text(&self.name, "nome", MAX_TEXT)?;
        check_text(&self.version, "versão", 64)?;
        check_text(&self.url, "url", MAX_URL)?;
        // Só HTTPS, sem credenciais embutidas nem espaços.
        let rest = self
            .url
            .strip_prefix("https://")
            .ok_or_else(|| CatalogError::BadEntry("url precisa ser https".into()))?;
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        if host.is_empty() || host.contains('@') || self.url.contains(char::is_whitespace) {
            return Err(CatalogError::BadEntry("url inválida".into()));
        }
        if parse_hex32(&self.sha256).is_none() {
            return Err(CatalogError::BadEntry("sha256 inválido".into()));
        }
        if self.size == 0
            || self.size > crate::MAX_WASM_BYTES as u64 + crate::MAX_MANIFEST_BYTES as u64 + 1024
        {
            return Err(CatalogError::BadEntry("tamanho fora do limite".into()));
        }
        Ok(())
    }
}

fn message(payload: &[u8]) -> Vec<u8> {
    [DOMAIN, payload].concat()
}

/// Assina um catálogo (usado pela ferramenta de publicação e pelos testes).
pub fn sign(key: &SigningKey, catalog: &Catalog) -> Result<Vec<u8>, CatalogError> {
    let payload =
        serde_json::to_string(catalog).map_err(|e| CatalogError::Malformed(e.to_string()))?;
    let sig = key.sign(&message(payload.as_bytes()));
    let env = Envelope {
        pubkey: to_hex(key.verifying_key().as_bytes()),
        sig: to_hex(&sig.to_bytes()),
        payload,
    };
    serde_json::to_vec(&env).map_err(|e| CatalogError::Malformed(e.to_string()))
}

/// Verifica estrutura, signatário confiável, assinatura, validade e cada entrada.
/// `now` = segundos Unix atuais (injetado para ser testável).
pub fn verify(bytes: &[u8], trust: &TrustStore, now: u64) -> Result<Catalog, CatalogError> {
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(CatalogError::TooLarge);
    }
    let env: Envelope =
        serde_json::from_slice(bytes).map_err(|e| CatalogError::Malformed(e.to_string()))?;
    let key = parse_hex32(&env.pubkey).ok_or_else(|| CatalogError::Malformed("pubkey".into()))?;
    if !trust.is_trusted(&key) {
        return Err(CatalogError::UntrustedSigner);
    }
    let sig_bytes = parse_sig(&env.sig).ok_or_else(|| CatalogError::Malformed("sig".into()))?;
    let vk = VerifyingKey::from_bytes(&key).map_err(|_| CatalogError::BadSignature)?;
    vk.verify_strict(
        &message(env.payload.as_bytes()),
        &Signature::from_bytes(&sig_bytes),
    )
    .map_err(|_| CatalogError::BadSignature)?;

    let cat: Catalog =
        serde_json::from_str(&env.payload).map_err(|e| CatalogError::Malformed(e.to_string()))?;
    if cat.format != 1 {
        return Err(CatalogError::Malformed("format".into()));
    }
    if now >= cat.expires_at || cat.issued_at > cat.expires_at {
        return Err(CatalogError::Expired);
    }
    if cat.entries.len() > MAX_ENTRIES {
        return Err(CatalogError::Malformed("entradas demais".into()));
    }
    let mut seen = std::collections::HashSet::new();
    for e in &cat.entries {
        e.validate()?;
        if !seen.insert(&e.id) {
            return Err(CatalogError::BadEntry("id repetido".into()));
        }
    }
    Ok(cat)
}

fn parse_sig(s: &str) -> Option<[u8; 64]> {
    if s.len() != 128 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 64];
    for (i, c) in s.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok()?;
    }
    Some(out)
}

/// O arquivo baixado precisa bater com o que o catálogo prometeu, antes de qualquer outra coisa.
pub fn check_download(entry: &Entry, bytes: &[u8]) -> bool {
    bytes.len() as u64 == entry.size && sha256_hex(bytes) == entry.sha256
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> SigningKey {
        SigningKey::from_bytes(&[n; 32])
    }
    fn trust(k: &SigningKey) -> TrustStore {
        TrustStore::new(vec![*k.verifying_key().as_bytes()])
    }
    fn entry(id: &str) -> Entry {
        Entry {
            id: id.into(),
            name: "App".into(),
            version: "1.0.0".into(),
            url: "https://example.com/a.sapp".into(),
            sha256: sha256_hex(b"x"),
            size: 10,
        }
    }
    fn cat(entries: Vec<Entry>) -> Catalog {
        Catalog {
            format: 1,
            issued_at: 100,
            expires_at: 1000,
            entries,
        }
    }

    #[test]
    fn valid_catalog_roundtrips() {
        let k = key(1);
        let bytes = sign(&k, &cat(vec![entry("clicker")])).unwrap();
        let c = verify(&bytes, &trust(&k), 500).unwrap();
        assert_eq!(c.entries.len(), 1);
    }

    #[test]
    fn untrusted_signer_rejected() {
        let bytes = sign(&key(1), &cat(vec![])).unwrap();
        assert_eq!(
            verify(&bytes, &trust(&key(2)), 500),
            Err(CatalogError::UntrustedSigner)
        );
    }

    #[test]
    fn expired_rejected() {
        let k = key(1);
        let bytes = sign(&k, &cat(vec![])).unwrap();
        assert_eq!(verify(&bytes, &trust(&k), 1000), Err(CatalogError::Expired));
    }

    #[test]
    fn any_payload_tamper_breaks_signature() {
        let k = key(1);
        let bytes = sign(&k, &cat(vec![entry("clicker")])).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let tampered = text.replace("example.com", "evil.test");
        assert_eq!(
            verify(tampered.as_bytes(), &trust(&k), 500),
            Err(CatalogError::BadSignature)
        );
    }

    #[test]
    fn signature_does_not_transfer_across_domains() {
        // Uma assinatura de manifesto (outro DOMAIN) sobre o mesmo texto não vale aqui.
        let k = key(1);
        let payload = serde_json::to_string(&cat(vec![])).unwrap();
        let sig = k.sign(&[b"superapp-sapp-v1\0", payload.as_bytes()].concat());
        let env = Envelope {
            pubkey: to_hex(k.verifying_key().as_bytes()),
            sig: to_hex(&sig.to_bytes()),
            payload,
        };
        let bytes = serde_json::to_vec(&env).unwrap();
        assert_eq!(
            verify(&bytes, &trust(&k), 500),
            Err(CatalogError::BadSignature)
        );
    }

    #[test]
    fn bad_entries_rejected() {
        let k = key(1);
        let t = trust(&k);
        let mut cases = Vec::new();
        let mut e = entry("clicker");
        e.url = "http://example.com/a.sapp".into();
        cases.push(e);
        let mut e = entry("clicker");
        e.url = "https://user:pw@example.com/a.sapp".into();
        cases.push(e);
        let mut e = entry("clicker");
        e.url = "https://exa mple.com/a".into();
        cases.push(e);
        let mut e = entry("clicker");
        e.sha256 = "zz".into();
        cases.push(e);
        let mut e = entry("clicker");
        e.size = 0;
        cases.push(e);
        cases.push(entry("../x"));
        for e in cases {
            let bytes = sign(&k, &cat(vec![e])).unwrap();
            assert!(
                matches!(verify(&bytes, &t, 500), Err(CatalogError::BadEntry(_))),
                "deveria rejeitar"
            );
        }
        let bytes = sign(&k, &cat(vec![entry("a"), entry("a")])).unwrap();
        assert!(matches!(
            verify(&bytes, &t, 500),
            Err(CatalogError::BadEntry(_))
        ));
    }

    #[test]
    fn garbage_and_oversize_rejected() {
        let t = TrustStore::default();
        assert!(verify(b"nope", &t, 0).is_err());
        assert_eq!(
            verify(&vec![b' '; MAX_CATALOG_BYTES + 1], &t, 0),
            Err(CatalogError::TooLarge)
        );
    }

    #[test]
    fn download_must_match_hash_and_size() {
        let mut e = entry("a");
        e.sha256 = sha256_hex(b"hello");
        e.size = 5;
        assert!(check_download(&e, b"hello"));
        assert!(!check_download(&e, b"hellp"));
        assert!(!check_download(&e, b"hello!"));
    }
}
