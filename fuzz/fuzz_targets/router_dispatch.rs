//! Roteador IPC: nome do comando, argumentos JSON e corpo binário arbitrários. Um comando
//! protegido por capability que a janela não tem nunca pode executar.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use superapp_core::{Caller, MemoryAudit, Policy, Router};

static RAN: AtomicBool = AtomicBool::new(false);
static ROUTER: OnceLock<Router> = OnceLock::new();

fn router() -> &'static Router {
    ROUTER.get_or_init(|| {
        let mut r = Router::new(
            Policy::new().grant("main", "ok"),
            Arc::new(MemoryAudit::default()),
        );
        r.register("secret", "forbidden", |_c, _a: serde_json::Value| {
            RAN.store(true, Ordering::SeqCst);
            Ok(1)
        });
        r.register_bytes("secret.bytes", "forbidden", 64, 64, |_c, b| {
            RAN.store(true, Ordering::SeqCst);
            Ok(b.to_vec())
        });
        r.register("typed", "ok", |_c, _a: serde_json::Map<String, serde_json::Value>| Ok(1));
        r
    })
}

fuzz_target!(|data: &[u8]| {
    // Divide a entrada: [janela(1)] [tamanho do nome(1)] nome... resto = args/corpo.
    if data.len() < 2 {
        return;
    }
    let windows = ["main", "untrusted", "mfe-a"];
    let caller = Caller::new(windows[data[0] as usize % windows.len()]);
    let n = (data[1] as usize).min(data.len() - 2);
    let name = String::from_utf8_lossy(&data[2..2 + n]).into_owned();
    let rest = &data[2 + n..];
    let r = router();
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(rest) {
        let _ = r.dispatch(&caller, &name, v);
    }
    let _ = r.dispatch_bytes(&caller, &name, rest);
    // Os comandos protegidos podem até existir, mas nunca executam sem a capability.
    assert!(!RAN.load(Ordering::SeqCst), "comando sem capability executou");
});
