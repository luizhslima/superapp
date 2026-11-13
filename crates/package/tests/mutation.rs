//! Fuzz por mutação dos parsers de entrada não confiável (pacote, manifesto, lista de chaves).
//! Determinístico (semente fixa) e só com `std`: roda em `cargo test` nos três SOs.
//!
//! Propriedades: (1) nenhuma entrada faz o verificador entrar em pânico; (2) **qualquer**
//! alteração de um pacote válido é rejeitada: `verify` só devolve `Ok` para o arquivo idêntico.
use ed25519_dalek::SigningKey;
use superapp_package::{pack, verify, Manifest, Permission, TrustStore};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[5; 32])
}

fn trust() -> TrustStore {
    TrustStore::new(vec![key().verifying_key().to_bytes()])
}

fn valid_package() -> Vec<u8> {
    let m = Manifest {
        schema: 1,
        id: "fuzz".into(),
        name: "Fuzz".into(),
        version: "1.2.3".into(),
        permissions: vec![Permission::Log, Permission::Storage],
        wasm_sha256: String::new(),
        limits: None,
        network: None,
    };
    let wasm: Vec<u8> = (0..400u32).map(|i| (i * 7 % 251) as u8).collect();
    pack(&key(), &m, &wasm).unwrap()
}

#[test]
fn every_single_bit_flip_of_a_valid_package_is_rejected() {
    let good = valid_package();
    assert!(verify(&good, &trust()).is_ok());
    for pos in 0..good.len() {
        for bit in 0..8 {
            let mut m = good.clone();
            m[pos] ^= 1 << bit;
            assert!(
                verify(&m, &trust()).is_err(),
                "alteração no byte {pos}, bit {bit} foi aceita"
            );
        }
    }
}

#[test]
fn random_mutations_never_panic_and_never_pass_unless_identical() {
    let good = valid_package();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for i in 0..30_000 {
        let mut m = good.clone();
        match rng.below(6) {
            0 => {
                // Vários bits aleatórios.
                for _ in 0..1 + rng.below(8) {
                    let p = rng.below(m.len());
                    m[p] ^= 1 << rng.below(8);
                }
            }
            1 => m.truncate(rng.below(m.len() + 1)),
            2 => {
                // Insere lixo no meio.
                let p = rng.below(m.len() + 1);
                let n = 1 + rng.below(32);
                let junk: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                m.splice(p..p, junk);
            }
            3 => {
                // Remove um trecho.
                let a = rng.below(m.len());
                let b = (a + 1 + rng.below(40)).min(m.len());
                m.drain(a..b);
            }
            4 => {
                // Corrompe os campos de tamanho do cabeçalho (101..109) com valores extremos.
                let v = [0u32, 1, 0x7fff_ffff, u32::MAX, 65_537][rng.below(5)];
                let off = if rng.below(2) == 0 { 101 } else { 105 };
                m[off..off + 4].copy_from_slice(&v.to_le_bytes());
            }
            _ => {
                // Bytes totalmente aleatórios.
                let n = rng.below(300);
                m = (0..n).map(|_| rng.next() as u8).collect();
            }
        }
        let r = verify(&m, &trust());
        if r.is_ok() {
            assert_eq!(m, good, "iteração {i}: pacote alterado foi aceito");
        }
    }
}

#[test]
fn manifest_json_and_trust_lists_never_panic_on_garbage() {
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    let base = br#"{"schema":1,"id":"a","name":"N","version":"1.0.0","permissions":["log"],"wasm_sha256":"0000000000000000000000000000000000000000000000000000000000000000"}"#;
    for _ in 0..30_000 {
        let mut b = base.to_vec();
        for _ in 0..1 + rng.below(6) {
            let p = rng.below(b.len());
            match rng.below(3) {
                0 => b[p] = rng.next() as u8,
                1 => {
                    b.remove(p);
                }
                _ => b.insert(p, rng.next() as u8),
            }
        }
        if let Ok(m) = serde_json::from_slice::<Manifest>(&b) {
            let _ = m.validate(); // não pode entrar em pânico
        }
        let text = String::from_utf8_lossy(&b);
        let _ = TrustStore::from_hex_lines(&text);
    }
}
