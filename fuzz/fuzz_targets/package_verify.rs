//! Qualquer sequência de bytes como pacote: `verify` nunca pode entrar em pânico.
//! A chave "confiável" é tirada da própria entrada (bytes 5..37), para o fuzzer alcançar a
//! checagem de assinatura em vez de parar sempre em "signatário não confiável".
#![no_main]
use libfuzzer_sys::fuzz_target;
use superapp_package::{verify, TrustStore};

fuzz_target!(|data: &[u8]| {
    let mut key = [0u8; 32];
    if data.len() >= 37 {
        key.copy_from_slice(&data[5..37]);
    }
    let trust = TrustStore::new(vec![key]);
    let _ = verify(data, &trust);
});
