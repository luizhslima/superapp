wit_bindgen::generate!({ world: "miniapp", path: "../wit" });

use spike::host::host_api::{log, slow_add};
use std::hint::black_box;

struct App;

impl Guest for App {
    fn run(n: u32) -> u32 {
        log("run chamado");
        slow_add(n, 1)
    }

    fn spin() {
        let mut x = 0u64;
        loop {
            x = black_box(x.wrapping_add(1));
        }
    }

    fn hog(mb: u32) -> u32 {
        let mut v: Vec<u8> = vec![1u8; (mb as usize) * 1024 * 1024];
        v[0] = 2;
        black_box(&v);
        v.len() as u32
    }

    fn try_fs() -> String {
        match std::fs::read_dir("/") {
            Ok(_) => "FS_ACESSIVEL".into(),
            Err(e) => format!("negado: {e}"),
        }
    }

    fn try_net() -> String {
        match std::net::TcpStream::connect("1.1.1.1:80") {
            Ok(_) => "REDE_ACESSIVEL".into(),
            Err(e) => format!("negado: {e}"),
        }
    }
}

export!(App);
