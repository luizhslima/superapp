//! Mini-app malicioso de teste: cada entrada de `run` é um ataque diferente.
wit_bindgen::generate!({ world: "miniapp", path: "../../../wit" });

use std::hint::black_box;
use superapp::host::{clock, log::log as host_log, storage};

struct App;

impl Guest for App {
    fn run(input: String) -> Result<String, String> {
        let (cmd, arg) = input.split_once(':').unwrap_or((input.as_str(), ""));
        match cmd {
            "echo" => {
                host_log("echo");
                Ok(arg.to_owned())
            }
            "fail" => Err(arg.to_owned()),
            // Laço infinito.
            "spin" => {
                let mut x = 0u64;
                loop {
                    x = black_box(x.wrapping_add(1));
                }
            }
            // Pede `arg` MiB de memória.
            "hog" => {
                let mb: usize = arg.parse().map_err(|_| "mb inválido".to_string())?;
                let mut v = vec![1u8; mb * 1024 * 1024];
                v[0] = 2;
                Ok(format!("{}", black_box(&v).len()))
            }
            // Acesso a FS e rede sem permissão.
            "fs" => Ok(match std::fs::read_dir("/") {
                Ok(_) => "FS_ACESSIVEL".into(),
                Err(e) => format!("negado: {e}"),
            }),
            "net" => Ok(match std::net::TcpStream::connect("1.1.1.1:80") {
                Ok(_) => "REDE_ACESSIVEL".into(),
                Err(e) => format!("negado: {e}"),
            }),
            // storage: set:chave=valor | get:chave | big (valor acima do limite)
            "set" => {
                let (k, v) = arg.split_once('=').ok_or("use chave=valor")?;
                storage::set(k, v.as_bytes())?;
                Ok("ok".into())
            }
            "get" => Ok(match storage::get(arg) {
                Some(v) => String::from_utf8_lossy(&v).into_owned(),
                None => "<vazio>".into(),
            }),
            "big" => {
                storage::set("big", &vec![0u8; 2 * 1024 * 1024])?;
                Ok("ok".into())
            }
            "now" => Ok(format!("{}", clock::now_ms())),
            _ => Err(format!("comando desconhecido: {cmd}")),
        }
    }
}

export!(App);
