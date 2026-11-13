//! Transporte real da rede mediada para mini-apps (ADR-011).
//!
//! A **política** (domínios, IPs, redirecionamentos, tamanhos) mora em
//! `superapp_wasm_host::net` e roda antes de qualquer byte sair; aqui só há DNS e HTTP:
//! - o DNS é resolvido pelo host (a política valida **todos** os IPs) e a conexão é **fixada**
//!   no IP validado (`resolve`), para um segundo DNS não poder trocar o destino (*rebinding*);
//! - redirecionamentos **não** são seguidos (a política os revalida e refaz o pedido);
//! - sem cookies, sem credenciais ambientes, sem proxy do sistema;
//! - o corpo é lido em pedaços e a leitura aborta ao passar do teto.
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use superapp_wasm_host::net::{BoxFuture, Method, NetBackend, Pinned, RawResponse, SendError};

pub struct ReqwestBackend;

impl NetBackend for ReqwestBackend {
    fn resolve<'a>(&'a self, host: &'a str) -> BoxFuture<'a, Result<Vec<IpAddr>, String>> {
        Box::pin(async move {
            let h = host.to_string();
            tauri::async_runtime::spawn_blocking(move || {
                (h.as_str(), 443)
                    .to_socket_addrs()
                    .map(|it| it.map(|a| a.ip()).collect::<Vec<_>>())
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|_| "não foi possível resolver o domínio".to_string())
        })
    }

    fn send<'a>(&'a self, r: Pinned<'a>) -> BoxFuture<'a, Result<RawResponse, SendError>> {
        Box::pin(send(r))
    }
}

/// Mensagem curta para o app: sem a URL (pode ter dados) e sem detalhes internos.
fn short(e: reqwest::Error) -> SendError {
    if e.is_timeout() {
        SendError::Timeout
    } else if e.is_connect() {
        SendError::Other("não foi possível conectar".into())
    } else {
        SendError::Other(format!("falha na requisição: {}", e.without_url()))
    }
}

async fn send(r: Pinned<'_>) -> Result<RawResponse, SendError> {
    // O provedor TLS é global; se já foi instalado (atualizador, loja), tudo bem.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let host = r
        .url
        .host_str()
        .ok_or_else(|| SendError::Other("sem domínio".into()))?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .resolve(
            host,
            SocketAddr::new(r.ip, r.url.port_or_known_default().unwrap_or(443)),
        )
        .no_proxy()
        .timeout(r.timeout)
        .user_agent("superapp-miniapp")
        .build()
        .map_err(|e| SendError::Other(e.to_string()))?;
    let mut rb = match r.method {
        Method::Get => client.get(r.url.as_str()),
        Method::Post => client.post(r.url.as_str()).body(r.body.to_vec()),
    };
    for (k, v) in r.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    let mut resp = rb.send().await.map_err(short)?;
    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let (status, content_type, location) = (
        resp.status().as_u16(),
        header("content-type").unwrap_or_default(),
        header("location"),
    );
    if resp
        .content_length()
        .is_some_and(|n| n > r.max_response as u64)
    {
        return Err(SendError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(short)? {
        if body.len() + chunk.len() > r.max_response {
            return Err(SendError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(RawResponse {
        status,
        content_type,
        location,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;
    use url::Url;

    /// Servidor HTTP mínimo (uma conexão): lê o pedido e responde com `resposta`.
    fn servidor(resposta: Vec<u8>, atraso: Duration) -> (u16, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let porta = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            std::thread::sleep(atraso);
            let _ = s.write_all(&resposta);
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        (porta, h)
    }

    fn http(status: &str, extra: &str, corpo: &[u8]) -> Vec<u8> {
        let mut v = format!(
            "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
            corpo.len()
        )
        .into_bytes();
        v.extend_from_slice(corpo);
        v
    }

    async fn pedir(
        url: &str,
        ip: &str,
        max: usize,
        timeout: Duration,
    ) -> Result<RawResponse, SendError> {
        let url = Url::parse(url).unwrap();
        send(Pinned {
            url: &url,
            ip: ip.parse().unwrap(),
            method: Method::Get,
            headers: &[("accept".into(), "text/plain".into())],
            body: &[],
            max_response: max,
            timeout,
        })
        .await
    }

    #[test]
    fn connects_to_the_pinned_ip_not_to_what_the_name_would_resolve_to() {
        let (porta, srv) = servidor(
            http("200 OK", "Content-Type: text/plain\r\n", b"oi"),
            Duration::ZERO,
        );
        // `fixado.invalid` não resolve em lugar nenhum: só chega se a conexão usar o IP fixado.
        let r = tauri::async_runtime::block_on(pedir(
            &format!("http://fixado.invalid:{porta}/caminho?q=1"),
            "127.0.0.1",
            1024,
            Duration::from_secs(5),
        ))
        .unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, b"oi".as_slice()));
        assert_eq!(r.content_type, "text/plain");
        let pedido = srv.join().unwrap();
        assert!(pedido.starts_with("GET /caminho?q=1 HTTP/1.1"), "{pedido}");
        assert!(pedido.to_lowercase().contains("host: fixado.invalid"));
        assert!(pedido.contains("superapp-miniapp"));
        assert!(!pedido.to_lowercase().contains("cookie"));
    }

    #[test]
    fn redirects_are_returned_not_followed() {
        let (porta, _srv) = servidor(
            http("302 Found", "Location: https://evil.example/x\r\n", b""),
            Duration::ZERO,
        );
        let r = tauri::async_runtime::block_on(pedir(
            &format!("http://a.invalid:{porta}/"),
            "127.0.0.1",
            1024,
            Duration::from_secs(5),
        ))
        .unwrap();
        assert_eq!(r.status, 302);
        assert_eq!(r.location.as_deref(), Some("https://evil.example/x"));
    }

    #[test]
    fn body_over_the_cap_aborts_the_read() {
        let grande = vec![b'a'; 5000];
        let (porta, _srv) = servidor(
            http("200 OK", "Content-Type: text/plain\r\n", &grande),
            Duration::ZERO,
        );
        let r = tauri::async_runtime::block_on(pedir(
            &format!("http://a.invalid:{porta}/"),
            "127.0.0.1",
            1000,
            Duration::from_secs(5),
        ));
        assert_eq!(r.err(), Some(SendError::TooLarge));
    }

    #[test]
    fn slow_server_hits_the_timeout() {
        let (porta, _srv) = servidor(
            http("200 OK", "Content-Type: text/plain\r\n", b"tarde"),
            Duration::from_secs(3),
        );
        let r = tauri::async_runtime::block_on(pedir(
            &format!("http://a.invalid:{porta}/"),
            "127.0.0.1",
            1000,
            Duration::from_millis(300),
        ));
        assert_eq!(r.err(), Some(SendError::Timeout));
    }

    #[test]
    fn refused_connection_gives_a_short_message_without_the_url() {
        // Porta fechada: o erro não pode vazar a URL (que pode carregar dados) para o app.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let porta = l.local_addr().unwrap().port();
        drop(l);
        let r = tauri::async_runtime::block_on(pedir(
            &format!("http://segredo.invalid:{porta}/token=abc"),
            "127.0.0.1",
            1000,
            Duration::from_secs(2),
        ));
        match r {
            Err(SendError::Other(m)) => {
                assert!(!m.contains("segredo") && !m.contains("token"), "{m}");
            }
            // No Windows, uma porta fechada pode estourar o tempo em vez de recusar na hora.
            Err(SendError::Timeout) => {}
            outro => panic!("esperava erro de conexão: {:?}", outro.err()),
        }
    }
}
