//! JSON arbitrário como manifesto, e texto arbitrário como lista de chaves confiáveis.
#![no_main]
use libfuzzer_sys::fuzz_target;
use superapp_package::{scan_secrets, Manifest, TrustStore};

fuzz_target!(|data: &[u8]| {
    if let Ok(m) = serde_json::from_slice::<Manifest>(data) {
        let _ = m.validate();
    }
    let _ = TrustStore::from_hex_lines(&String::from_utf8_lossy(data));
    let _ = scan_secrets(data);
});
