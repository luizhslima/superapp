//! Pacote de mini-app (`.sapp`): container mínimo + manifesto + assinatura Ed25519 (ADR-010).
//!
//! Layout (inteiros em little-endian):
//! ```text
//! "SAPP" | versão(1) | chave pública(32) | assinatura(64) | len_manifesto(4) | len_wasm(4)
//! | manifesto (JSON) | wasm
//! ```
//! A assinatura cobre `DOMAIN || bytes exatos do manifesto`; o manifesto carrega o SHA-256
//! do wasm. Verificação: estrutura → signatário confiável → assinatura → manifesto → hash.
//! Todo tamanho é checado contra tetos **antes** de qualquer alocação ou fatiamento.

pub mod manifest;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

pub use manifest::{valid_app_id, Manifest, Permission, RequestedLimits};

const MAGIC: &[u8; 4] = b"SAPP";
const FORMAT_VERSION: u8 = 1;
const HEADER_LEN: usize = 4 + 1 + 32 + 64 + 4 + 4;
/// Separação de domínio: uma assinatura de manifesto não vale para outro uso da mesma chave.
const DOMAIN: &[u8] = b"superapp-sapp-v1\0";

pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_WASM_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PackageError {
    #[error("pacote malformado: {0}")]
    Malformed(&'static str),
    #[error("versão de formato não suportada")]
    UnsupportedVersion,
    #[error("signatário não é confiável")]
    UntrustedSigner,
    #[error("assinatura inválida")]
    BadSignature,
    #[error("manifesto inválido: {0}")]
    InvalidManifest(String),
    #[error("o código não bate com o hash do manifesto")]
    HashMismatch,
}

/// Chaves públicas de autores em quem o host confia.
#[derive(Debug, Clone, Default)]
pub struct TrustStore {
    keys: Vec<[u8; 32]>,
}

impl TrustStore {
    pub fn new(keys: Vec<[u8; 32]>) -> Self {
        Self { keys }
    }

    /// Uma chave pública por linha, em hexadecimal; `#` começa comentário.
    pub fn from_hex_lines(text: &str) -> Result<Self, String> {
        let mut keys = Vec::new();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            keys.push(parse_hex32(line).ok_or_else(|| format!("chave inválida: {line}"))?);
        }
        Ok(Self { keys })
    }

    pub fn is_trusted(&self, key: &[u8; 32]) -> bool {
        self.keys.contains(key)
    }
}

/// Pacote que passou por toda a verificação.
#[derive(Debug)]
pub struct VerifiedPackage {
    pub manifest: Manifest,
    pub wasm: Vec<u8>,
    pub signer: [u8; 32],
}

pub fn sha256_hex(data: &[u8]) -> String {
    to_hex(&Sha256::digest(data))
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(out)
}

fn signed_message(manifest_bytes: &[u8]) -> Vec<u8> {
    [DOMAIN, manifest_bytes].concat()
}

/// Monta e assina um pacote. O `wasm_sha256` do manifesto é recalculado a partir do `wasm`.
pub fn pack(key: &SigningKey, manifest: &Manifest, wasm: &[u8]) -> Result<Vec<u8>, PackageError> {
    let mut m = manifest.clone();
    m.wasm_sha256 = sha256_hex(wasm);
    m.validate()?;
    if wasm.len() > MAX_WASM_BYTES {
        return Err(PackageError::Malformed("wasm acima do teto"));
    }
    let manifest_bytes =
        serde_json::to_vec(&m).map_err(|e| PackageError::InvalidManifest(e.to_string()))?;
    if manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(PackageError::Malformed("manifesto acima do teto"));
    }
    let sig = key.sign(&signed_message(&manifest_bytes));
    let mut out = Vec::with_capacity(HEADER_LEN + manifest_bytes.len() + wasm.len());
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(key.verifying_key().as_bytes());
    out.extend_from_slice(&sig.to_bytes());
    out.extend_from_slice(&(manifest_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&(wasm.len() as u32).to_le_bytes());
    out.extend_from_slice(&manifest_bytes);
    out.extend_from_slice(wasm);
    Ok(out)
}

/// Verifica um pacote vindo de fonte não confiável.
pub fn verify(bytes: &[u8], trust: &TrustStore) -> Result<VerifiedPackage, PackageError> {
    if bytes.len() < HEADER_LEN {
        return Err(PackageError::Malformed("curto demais"));
    }
    if &bytes[0..4] != MAGIC {
        return Err(PackageError::Malformed("assinatura de formato incorreta"));
    }
    if bytes[4] != FORMAT_VERSION {
        return Err(PackageError::UnsupportedVersion);
    }
    let signer: [u8; 32] = bytes[5..37].try_into().expect("tamanho fixo");
    let sig_bytes: [u8; 64] = bytes[37..101].try_into().expect("tamanho fixo");
    let m_len = u32::from_le_bytes(bytes[101..105].try_into().expect("4 bytes")) as usize;
    let w_len = u32::from_le_bytes(bytes[105..109].try_into().expect("4 bytes")) as usize;
    if m_len > MAX_MANIFEST_BYTES || w_len > MAX_WASM_BYTES {
        return Err(PackageError::Malformed("tamanho declarado acima do teto"));
    }
    // Soma sem estouro; o total precisa ser exato (sem bytes sobrando nem faltando).
    let total = HEADER_LEN
        .checked_add(m_len)
        .and_then(|v| v.checked_add(w_len))
        .ok_or(PackageError::Malformed("tamanho inconsistente"))?;
    if bytes.len() != total {
        return Err(PackageError::Malformed(
            "tamanho não confere com o cabeçalho",
        ));
    }
    let manifest_bytes = &bytes[HEADER_LEN..HEADER_LEN + m_len];
    let wasm = &bytes[HEADER_LEN + m_len..];

    if !trust.is_trusted(&signer) {
        return Err(PackageError::UntrustedSigner);
    }
    let vk = VerifyingKey::from_bytes(&signer).map_err(|_| PackageError::BadSignature)?;
    vk.verify_strict(
        &signed_message(manifest_bytes),
        &Signature::from_bytes(&sig_bytes),
    )
    .map_err(|_| PackageError::BadSignature)?;

    let manifest: Manifest = serde_json::from_slice(manifest_bytes)
        .map_err(|e| PackageError::InvalidManifest(e.to_string()))?;
    manifest.validate()?;
    if sha256_hex(wasm) != manifest.wasm_sha256 {
        return Err(PackageError::HashMismatch);
    }
    Ok(VerifiedPackage {
        manifest,
        wasm: wasm.to_vec(),
        signer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn trust(k: &SigningKey) -> TrustStore {
        TrustStore::new(vec![k.verifying_key().to_bytes()])
    }

    fn manifest() -> Manifest {
        Manifest {
            schema: 1,
            id: "demo".into(),
            name: "Demo".into(),
            version: "1.0.0".into(),
            permissions: vec![Permission::Log, Permission::Storage],
            wasm_sha256: String::new(), // recalculado por `pack`
            limits: None,
        }
    }

    const WASM: &[u8] = b"\0asm-fake-code";

    fn good() -> (Vec<u8>, SigningKey) {
        let k = key(7);
        (pack(&k, &manifest(), WASM).unwrap(), k)
    }

    #[test]
    fn roundtrip_verifies_and_returns_contents() {
        let (pkg, k) = good();
        let v = verify(&pkg, &trust(&k)).unwrap();
        assert_eq!(v.manifest.id, "demo");
        assert_eq!(v.wasm, WASM);
        assert_eq!(v.signer, k.verifying_key().to_bytes());
        assert_eq!(v.manifest.wasm_sha256, sha256_hex(WASM));
    }

    #[test]
    fn tampered_manifest_is_rejected() {
        let (mut pkg, k) = good();
        // Troca uma letra do nome dentro do JSON do manifesto.
        let pos = pkg.windows(4).position(|w| w == b"Demo").unwrap();
        pkg[pos] = b'X';
        assert_eq!(
            verify(&pkg, &trust(&k)).unwrap_err(),
            PackageError::BadSignature
        );
    }

    #[test]
    fn tampered_wasm_is_rejected_by_hash() {
        let (mut pkg, k) = good();
        let last = pkg.len() - 1;
        pkg[last] ^= 1;
        assert_eq!(
            verify(&pkg, &trust(&k)).unwrap_err(),
            PackageError::HashMismatch
        );
    }

    #[test]
    fn corrupted_signature_is_rejected() {
        let (mut pkg, k) = good();
        pkg[40] ^= 1; // dentro dos 64 bytes da assinatura
        assert_eq!(
            verify(&pkg, &trust(&k)).unwrap_err(),
            PackageError::BadSignature
        );
    }

    #[test]
    fn valid_signature_from_untrusted_key_is_rejected() {
        let (pkg, _) = good();
        let outro = key(9);
        assert_eq!(
            verify(&pkg, &trust(&outro)).unwrap_err(),
            PackageError::UntrustedSigner
        );
    }

    #[test]
    fn swapping_signer_key_for_trusted_one_breaks_the_signature() {
        // Atacante assina com a própria chave e troca a chave pública pela de um autor confiável.
        let atacante = key(66);
        let confiavel = key(7);
        let mut pkg = pack(&atacante, &manifest(), WASM).unwrap();
        pkg[5..37].copy_from_slice(confiavel.verifying_key().as_bytes());
        assert_eq!(
            verify(&pkg, &trust(&confiavel)).unwrap_err(),
            PackageError::BadSignature
        );
    }

    #[test]
    fn signature_over_manifest_without_domain_prefix_is_not_accepted() {
        let k = key(7);
        let (pkg, _) = good();
        let m_len = u32::from_le_bytes(pkg[101..105].try_into().unwrap()) as usize;
        let manifest_bytes = pkg[HEADER_LEN..HEADER_LEN + m_len].to_vec();
        let mut forged = pkg.clone();
        forged[37..101].copy_from_slice(&k.sign(&manifest_bytes).to_bytes()); // sem DOMAIN
        assert_eq!(
            verify(&forged, &trust(&k)).unwrap_err(),
            PackageError::BadSignature
        );
    }

    #[test]
    fn wasm_swapped_for_another_valid_hash_is_rejected() {
        // Cabeçalho e manifesto originais, mas com o código de outro pacote.
        let (pkg, k) = good();
        let mut bad = pkg[..pkg.len() - WASM.len()].to_vec();
        bad.extend_from_slice(b"\0asm-other-xxx");
        assert_eq!(bad.len(), pkg.len());
        assert_eq!(
            verify(&bad, &trust(&k)).unwrap_err(),
            PackageError::HashMismatch
        );
    }

    #[test]
    fn malformed_containers_are_rejected_without_panicking() {
        let (pkg, k) = good();
        let t = trust(&k);
        assert!(matches!(verify(&[], &t), Err(PackageError::Malformed(_))));
        assert!(matches!(
            verify(&pkg[..50], &t),
            Err(PackageError::Malformed(_))
        ));
        let mut m = pkg.clone();
        m[0] = b'X';
        assert!(matches!(verify(&m, &t), Err(PackageError::Malformed(_))));
        let mut v = pkg.clone();
        v[4] = 2;
        assert_eq!(
            verify(&v, &t).unwrap_err(),
            PackageError::UnsupportedVersion
        );
        // Bytes sobrando e faltando.
        let mut extra = pkg.clone();
        extra.push(0);
        assert!(matches!(
            verify(&extra, &t),
            Err(PackageError::Malformed(_))
        ));
        assert!(matches!(
            verify(&pkg[..pkg.len() - 1], &t),
            Err(PackageError::Malformed(_))
        ));
        // Tamanhos declarados absurdos (4 GiB) são barrados antes de qualquer alocação.
        let mut huge = pkg.clone();
        huge[105..109].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(verify(&huge, &t), Err(PackageError::Malformed(_))));
        let mut huge_m = pkg;
        huge_m[101..105].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            verify(&huge_m, &t),
            Err(PackageError::Malformed(_))
        ));
    }

    #[test]
    fn invalid_manifests_cannot_be_packed_or_verified() {
        let k = key(7);
        let mut bad = vec![];
        let mut m = manifest();
        m.id = "../x".into();
        bad.push(m);
        let mut m = manifest();
        m.permissions = vec![Permission::Log, Permission::Log];
        bad.push(m);
        let mut m = manifest();
        m.version = "1.0".into();
        bad.push(m);
        let mut m = manifest();
        m.name = "a\nb".into();
        bad.push(m);
        let mut m = manifest();
        m.schema = 2;
        bad.push(m);
        let mut m = manifest();
        m.limits = Some(RequestedLimits {
            fuel: Some(u64::MAX),
            ..Default::default()
        });
        bad.push(m);
        for m in bad {
            assert!(
                matches!(pack(&k, &m, WASM), Err(PackageError::InvalidManifest(_))),
                "deveria recusar {m:?}"
            );
        }
    }

    #[test]
    fn signed_manifest_with_unknown_field_or_permission_is_rejected() {
        // Manifesto assinado de verdade (o autor é confiável), mas com campo/permisão que
        // este host não conhece: precisa falhar, não ser ignorado.
        let k = key(7);
        for json in [
            r#"{"schema":1,"id":"demo","name":"D","version":"1.0.0","permissions":["log"],"wasm_sha256":"HASH","extra":1}"#,
            r#"{"schema":1,"id":"demo","name":"D","version":"1.0.0","permissions":["network"],"wasm_sha256":"HASH"}"#,
        ] {
            let json = json.replace("HASH", &sha256_hex(WASM));
            let bytes = json.as_bytes();
            let sig = k.sign(&signed_message(bytes));
            let mut pkg = Vec::new();
            pkg.extend_from_slice(MAGIC);
            pkg.push(FORMAT_VERSION);
            pkg.extend_from_slice(k.verifying_key().as_bytes());
            pkg.extend_from_slice(&sig.to_bytes());
            pkg.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            pkg.extend_from_slice(&(WASM.len() as u32).to_le_bytes());
            pkg.extend_from_slice(bytes);
            pkg.extend_from_slice(WASM);
            assert!(
                matches!(
                    verify(&pkg, &trust(&k)),
                    Err(PackageError::InvalidManifest(_))
                ),
                "{json}"
            );
        }
    }

    #[test]
    fn trust_store_parses_hex_lines_and_rejects_garbage() {
        let k = key(7);
        let hex = to_hex(k.verifying_key().as_bytes());
        let t = TrustStore::from_hex_lines(&format!("# autores\n{hex} # eu\n\n")).unwrap();
        assert!(t.is_trusted(&k.verifying_key().to_bytes()));
        assert!(TrustStore::from_hex_lines("zz").is_err());
        assert!(TrustStore::from_hex_lines(&hex[..62]).is_err());
    }
}
