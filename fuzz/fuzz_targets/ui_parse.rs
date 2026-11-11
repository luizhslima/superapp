//! Saída de UI arbitrária de um mini-app: o validador não pode entrar em pânico e tudo que ele
//! aceita precisa respeitar os limites.
#![no_main]
use libfuzzer_sys::fuzz_target;
use superapp_ui::{Event, Node, MAX_OUTPUT_BYTES};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if Node::parse(&text).is_ok() {
        assert!(text.len() <= MAX_OUTPUT_BYTES);
    }
    if let Ok(e) = serde_json::from_str::<Event>(&text) {
        let _ = e.validate();
    }
});
