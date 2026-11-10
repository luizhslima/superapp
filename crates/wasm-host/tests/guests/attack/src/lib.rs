//! Mini-app malicioso de teste: cada entrada de `run` é um ataque diferente.
wit_bindgen::generate!({ world: "miniapp", path: "../../../wit" });

use std::hint::black_box;
use superapp::host::{clock, log::log as host_log, storage};

struct App;

impl Guest for App {
    fn run(input: String) -> Result<String, String> {
        // Eventos de UI (JSON): o "modo" malicioso vem do storage, para os testes escolherem.
        if input.starts_with('{') {
            return ui_mode();
        }
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

/// Responde a eventos de UI conforme o valor da chave `mode` no storage.
fn ui_mode() -> Result<String, String> {
    let mode = storage::get("mode")
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default();
    match mode.as_str() {
        "ok" => Ok(r#"{"type":"text","text":"tudo certo"}"#.into()),
        // HTML/script disfarçado de UI.
        "html" => Ok(r#"{"type":"html","html":"<script>alert(1)</script>"}"#.into()),
        // Campo de estilo que o catálogo não conhece.
        "style" => Ok(r#"{"type":"text","text":"x","style":"position:fixed"}"#.into()),
        // Saída gigantesca (acima de 64 KiB).
        "huge" => Ok(format!(
            r#"{{"type":"text","text":"{}"}}"#,
            "x".repeat(70 * 1024)
        )),
        // Aninhamento acima do limite de profundidade.
        "deep" => {
            let mut s = String::from(r#"{"type":"text","text":"x"}"#);
            for _ in 0..12 {
                s = format!(r#"{{"type":"stack","children":[{s}]}}"#);
            }
            Ok(s)
        }
        "notjson" => Ok("<b>oi</b>".into()),
        // Controle dentro do texto.
        "control" => Ok("{\"type\":\"text\",\"text\":\"a\\u0007b\"}".into()),
        "panic" => panic!("falha deliberada do mini-app"),
        "spin" => {
            let mut x = 0u64;
            loop {
                x = black_box(x.wrapping_add(1));
            }
        }
        "hog" => {
            let mut v = vec![1u8; 64 * 1024 * 1024];
            v[0] = 2;
            Ok(format!("{}", black_box(&v).len()))
        }
        _ => Err("modo desconhecido".into()),
    }
}

export!(App);
