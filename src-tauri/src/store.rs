//! Catálogo e download de mini-apps (E5.1). É a única rede do host além do atualizador.
//!
//! Regras (todas testadas):
//! - O endereço do catálogo é uma constante; a UI nunca informa URL. O download só usa a URL de
//!   uma entrada do catálogo **já verificado** (assinatura de chave confiável, validade).
//! - Só `https://`. Redirecionamentos também só para `https://`, no máximo 5.
//! - Teto de bytes aplicado durante a leitura (o corpo nunca passa do limite, mesmo que o
//!   servidor minta no `Content-Length`), com timeout total.
//! - O arquivo baixado precisa bater com o SHA-256 e o tamanho prometidos pelo catálogo e,
//!   depois, passa pela verificação normal do pacote (`stage_bytes`) e pelo consentimento.
//! - Nada é instalado sozinho.
use crate::apps::AppsState;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use superapp_core::{ErrorCode, IpcError, Router};
use superapp_package::catalog::{self, Catalog, MAX_CATALOG_BYTES};

/// Capability dos comandos da loja. Só a janela principal a recebe.
pub const CAP: &str = "store:use";
/// Onde o catálogo assinado é publicado (arquivo estático do repositório).
pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/luizhslima/superapp/main/catalog/catalog.json";

#[derive(Default)]
pub struct StoreState {
    /// Último catálogo verificado: o `store.download` só aceita ids que estão nele.
    catalog: Mutex<Option<Catalog>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DownloadArgs {
    id: String,
}

fn fail(code: ErrorCode, msg: impl std::fmt::Display) -> IpcError {
    IpcError::new(code, format!("loja: {msg}"))
}

pub fn check_https(url: &str) -> Result<(), String> {
    if url.starts_with("https://") {
        Ok(())
    } else {
        Err("só https é permitido".into())
    }
}

fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    // O provedor TLS é global; se já foi instalado (pelo atualizador), tudo bem.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            attempt.error("redirecionamentos demais")
        } else if attempt.url().scheme() != "https" {
            attempt.error("redirecionamento para fora de https")
        } else {
            attempt.follow()
        }
    });
    reqwest::Client::builder()
        .redirect(policy)
        .timeout(timeout)
        .user_agent("superapp")
        .build()
        .map_err(|e| e.to_string())
}

/// Lê o corpo respeitando um teto de bytes durante a leitura.
async fn fetch_limited(client: &reqwest::Client, url: &str, max: u64) -> Result<Vec<u8>, String> {
    let mut resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("servidor respondeu {}", resp.status().as_u16()));
    }
    if resp.content_length().is_some_and(|n| n > max) {
        return Err("resposta maior que o limite".into());
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        if out.len() as u64 + chunk.len() as u64 > max {
            return Err("resposta maior que o limite".into());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn register(r: &mut Router, apps: Arc<AppsState>) {
    let state = Arc::new(StoreState::default());

    let (s, a) = (state.clone(), apps.clone());
    r.register("store.catalog", CAP, move |_c, _a: Value| {
        tauri::async_runtime::block_on(async {
            let c = client(Duration::from_secs(20)).map_err(|e| fail(ErrorCode::Internal, e))?;
            check_https(CATALOG_URL).map_err(|e| fail(ErrorCode::Internal, e))?;
            let bytes = fetch_limited(&c, CATALOG_URL, MAX_CATALOG_BYTES as u64)
                .await
                .map_err(|e| fail(ErrorCode::Internal, e))?;
            let cat = catalog::verify(&bytes, a.trust(), now_secs())
                .map_err(|e| fail(ErrorCode::PermissionDenied, e))?;
            let out = json!({ "expires_at": cat.expires_at, "entries": cat.entries });
            *s.catalog.lock().expect("catalog") = Some(cat);
            Ok(out)
        })
    });

    r.register("store.download", CAP, move |_c, a: DownloadArgs| {
        let entry = {
            let g = state.catalog.lock().expect("catalog");
            let cat = g
                .as_ref()
                .ok_or_else(|| fail(ErrorCode::InvalidArgs, "carregue o catálogo antes"))?;
            cat.entries
                .iter()
                .find(|e| e.id == a.id)
                .cloned()
                .ok_or_else(|| fail(ErrorCode::NotFound, "id fora do catálogo"))?
        };
        tauri::async_runtime::block_on(async {
            check_https(&entry.url).map_err(|e| fail(ErrorCode::Internal, e))?;
            let c = client(Duration::from_secs(120)).map_err(|e| fail(ErrorCode::Internal, e))?;
            let bytes = fetch_limited(&c, &entry.url, entry.size)
                .await
                .map_err(|e| fail(ErrorCode::Internal, e))?;
            if !catalog::check_download(&entry, &bytes) {
                return Err(fail(
                    ErrorCode::PermissionDenied,
                    "arquivo não confere com o catálogo (hash/tamanho)",
                ));
            }
            // Mesma verificação de um arquivo arrastado: assinatura do autor, manifesto, hash.
            apps.stage_bytes(bytes)
                .map_err(|e| fail(ErrorCode::InvalidArgs, e))?;
            Ok(Value::Null)
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Servidor HTTP mínimo: responde `raw` (resposta inteira) a cada conexão.
    fn serve(raw: Vec<u8>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { break };
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let _ = s.write_all(&raw);
            }
        });
        format!("http://{addr}/x")
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        let mut v = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        v.extend_from_slice(body);
        v
    }

    fn get(url: &str, max: u64) -> Result<Vec<u8>, String> {
        let c = client(Duration::from_secs(5)).unwrap();
        tauri::async_runtime::block_on(fetch_limited(&c, url, max))
    }

    #[test]
    fn only_https_urls_pass() {
        assert!(check_https("https://example.com/a").is_ok());
        assert!(check_https("http://example.com/a").is_err());
        assert!(check_https("file:///etc/passwd").is_err());
        assert!(check_https("ftp://x").is_err());
    }

    #[test]
    fn reads_a_body_within_the_limit() {
        let url = serve(ok(b"hello"));
        assert_eq!(get(&url, 5).unwrap(), b"hello");
    }

    #[test]
    fn rejects_a_body_over_the_limit_by_content_length() {
        let url = serve(ok(b"hello world"));
        assert!(get(&url, 5).is_err());
    }

    #[test]
    fn rejects_a_body_over_the_limit_even_if_the_server_lies() {
        // Content-Length pequeno mas corpo maior não passa o teto: o leitor corta.
        let mut raw = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
        raw.extend_from_slice(&[b'a'; 4096]); // sem Content-Length: lido até fechar
        let url = serve(raw);
        assert!(get(&url, 100).is_err());
    }

    #[test]
    fn error_status_is_an_error() {
        let url = serve(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        );
        assert!(get(&url, 100).is_err());
    }

    #[test]
    fn redirect_out_of_https_is_blocked() {
        let url = serve(
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
        );
        let err = get(&url, 100).unwrap_err();
        assert!(!err.is_empty());
    }
}
