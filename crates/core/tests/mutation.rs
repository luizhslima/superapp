//! Fuzz do roteador IPC: nomes de comando, argumentos JSON e corpos binários arbitrários.
//! Propriedades: nunca entra em pânico; sem capability, **nada** executa, qualquer que seja a
//! entrada; limites de tamanho binário valem sempre.
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use superapp_core::{Caller, ErrorCode, IpcError, MemoryAudit, Policy, Router};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn string(&mut self, max: usize) -> String {
        let n = self.below(max);
        (0..n)
            .map(|_| char::from_u32(self.next() as u32 % 0x2000).unwrap_or('?'))
            .collect()
    }
    /// JSON aleatório pequeno (objetos, listas, números, textos, nulos), com profundidade limitada.
    fn json(&mut self, depth: usize) -> Value {
        match self.below(if depth == 0 { 4 } else { 6 }) {
            0 => Value::Null,
            1 => json!(self.next() as i64),
            2 => Value::String(self.string(20)),
            3 => Value::Bool(self.below(2) == 0),
            4 => Value::Array((0..self.below(4)).map(|_| self.json(depth - 1)).collect()),
            _ => Value::Object(
                (0..self.below(4))
                    .map(|_| (self.string(6), self.json(depth - 1)))
                    .collect(),
            ),
        }
    }
}

fn router(executed: Arc<AtomicUsize>) -> Router {
    let policy = Policy::new().grant("main", "ok");
    let mut r = Router::new(policy, Arc::new(MemoryAudit::default()));
    let e = executed.clone();
    r.register("secret.cmd", "forbidden", move |_c, _a: Value| {
        e.fetch_add(1, Ordering::SeqCst);
        Ok(json!(1))
    });
    let e = executed.clone();
    r.register(
        "typed.cmd",
        "ok",
        move |_c, a: serde_json::Map<String, Value>| {
            e.fetch_add(1, Ordering::SeqCst);
            if a.len() > 3 {
                return Err(IpcError::new(ErrorCode::InvalidArgs, "muitos campos"));
            }
            Ok(json!(a.len()))
        },
    );
    let e = executed.clone();
    r.register_bytes("secret.bytes", "forbidden", 8, 8, move |_c, b| {
        e.fetch_add(1, Ordering::SeqCst);
        Ok(b.to_vec())
    });
    r.register_bytes("echo.bytes", "ok", 16, 16, move |_c, b| {
        executed.fetch_add(1, Ordering::SeqCst);
        Ok(b.to_vec())
    });
    r
}

#[test]
fn arbitrary_commands_and_args_never_panic_and_never_bypass_the_policy() {
    let executed = Arc::new(AtomicUsize::new(0));
    let r = router(executed.clone());
    let mut rng = Rng(0xC0FF_EE00_1234_5678);
    let windows = ["main", "untrusted", "mfe-a", "", "main\0", "MAIN"];
    let mut forbidden_ran = false;
    for _ in 0..50_000 {
        let caller = Caller::new(windows[rng.below(windows.len())]);
        let name = match rng.below(4) {
            0 => "secret.cmd".to_owned(),
            1 => "typed.cmd".to_owned(),
            2 => rng.string(70),
            _ => format!(
                "{}{}",
                ["secret", "typed", "echo"][rng.below(3)],
                rng.string(3)
            ),
        };
        let before = executed.load(Ordering::SeqCst);
        let res = r.dispatch(&caller, &name, rng.json(3));
        // Comando protegido nunca executa, para nenhuma janela.
        if name == "secret.cmd" && res.is_ok() {
            forbidden_ran = true;
        }
        // Se algo executou, foi o comando permitido e só para a janela com a capability.
        if executed.load(Ordering::SeqCst) != before {
            assert_eq!(name, "typed.cmd");
            assert_eq!(caller.window, "main");
        }
    }
    assert!(!forbidden_ran, "comando sem capability executou");
}

#[test]
fn arbitrary_binary_bodies_respect_limits_and_policy() {
    let executed = Arc::new(AtomicUsize::new(0));
    let r = router(executed.clone());
    let mut rng = Rng(0xABCD_EF01_2345_6789);
    for _ in 0..50_000 {
        let n = rng.below(40);
        let body: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        let (name, who) = (
            ["echo.bytes", "secret.bytes", "nope", ""][rng.below(4)],
            ["main", "untrusted"][rng.below(2)],
        );
        let res = r.dispatch_bytes(&Caller::new(who), name, &body);
        match (name, who) {
            ("echo.bytes", "main") if body.len() <= 16 => assert_eq!(res.unwrap(), body),
            ("echo.bytes", "main") => {
                assert_eq!(res.unwrap_err().code, ErrorCode::InvalidArgs)
            }
            _ => assert!(res.is_err(), "{name}/{who} não deveria ter sucesso"),
        }
    }
    // O comando binário protegido nunca executou; só o permitido, com corpo dentro do limite.
    assert!(executed.load(Ordering::SeqCst) > 0);
}
