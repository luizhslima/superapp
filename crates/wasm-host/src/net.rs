//! Rede mediada para mini-apps (ADR-011).
//!
//! O guest **nunca** abre conexão: ele pede `fetch` ao host, e o host aplica as regras aqui
//! antes de qualquer byte sair. Este módulo é só política e orquestração; o transporte real
//! (DNS, TLS, HTTP) entra por [`NetBackend`], e por isso tudo é testável sem internet.
//!
//! Regras (todas com teste de negação):
//! - só `https`, métodos `GET`/`POST`, porta 443, sem usuário/senha, sem IP literal;
//! - o domínio precisa estar na lista aprovada do manifesto (exato ou `*.dominio`, um nível);
//! - o DNS é resolvido pelo host e **todos** os IPs devem ser públicos (loopback, redes privadas,
//!   link-local, metadata de nuvem, CGNAT etc. são barrados); a conexão usa o IP validado
//!   (contra *DNS rebinding*);
//! - cada redirecionamento é revalidado do zero (máx. 3);
//! - corpo enviado ≤ 64 KiB, resposta ≤ 1 MiB (erro, nunca truncada em silêncio), só texto/JSON/XML;
//! - cabeçalhos: só `accept` e `content-type`; sem cookies nem credenciais ambientes;
//! - no máximo 4 chamadas por execução e 30 por minuto por app; 10 s por chamada.
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use url::{Host, Url};

pub const MAX_CALLS_PER_RUN: u32 = 4;
pub const MAX_CALLS_PER_MINUTE: usize = 30;
pub const MAX_REQUEST_BODY: usize = 64 * 1024;
pub const MAX_RESPONSE_BODY: usize = 1024 * 1024;
pub const MAX_REDIRECTS: usize = 3;
pub const MAX_URL_LEN: usize = 2048;
pub const MAX_HEADERS: usize = 4;
pub const MAX_HEADER_VALUE: usize = 200;
pub const CALL_TIMEOUT: Duration = Duration::from_secs(10);

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// Pedido do guest, ainda não confiável.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Motivos de recusa. Todos viram auditoria `DeniedByNetworkRule`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    #[error("rede indisponível neste host")]
    NoBackend,
    #[error("limite de {MAX_CALLS_PER_RUN} chamadas por execução")]
    CallsPerRun,
    #[error("limite de {MAX_CALLS_PER_MINUTE} chamadas por minuto")]
    CallsPerMinute,
    #[error("método não permitido (use GET ou POST)")]
    Method,
    #[error("URL inválida: {0}")]
    BadUrl(&'static str),
    #[error("domínio fora da lista aprovada")]
    HostNotAllowed,
    #[error("endereço de rede não público")]
    IpBlocked,
    #[error("cabeçalho não permitido")]
    Header,
    #[error("corpo enviado maior que {MAX_REQUEST_BODY} bytes")]
    RequestTooLarge,
    #[error("resposta maior que {MAX_RESPONSE_BODY} bytes")]
    ResponseTooLarge,
    #[error("tipo de conteúdo não permitido (só texto, JSON ou XML)")]
    ContentType,
    #[error("redirecionamentos demais")]
    TooManyRedirects,
    #[error("tempo limite da chamada")]
    Timeout,
    #[error("falha de rede: {0}")]
    Transport(String),
}

impl NetError {
    /// `false` para falhas de transporte (o servidor, não a regra, recusou).
    pub fn is_rule(&self) -> bool {
        !matches!(self, NetError::Transport(_) | NetError::Timeout)
    }
}

/// Erro do transporte (o backend também aplica o teto de resposta enquanto lê).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    TooLarge,
    Timeout,
    Other(String),
}

/// Pedido já validado, com o IP escolhido pelo host (o backend **deve** conectar nele).
pub struct Pinned<'a> {
    pub url: &'a Url,
    pub ip: IpAddr,
    pub method: Method,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
    pub max_response: usize,
    pub timeout: Duration,
}

pub struct RawResponse {
    pub status: u16,
    pub content_type: String,
    pub location: Option<String>,
    pub body: Vec<u8>,
}

/// Transporte e DNS reais. Fica fora deste crate (o host Tauri usa `reqwest`); testes usam um
/// falso. O backend **não** segue redirecionamentos e **não** guarda cookies.
pub trait NetBackend: Send + Sync {
    fn resolve<'a>(&'a self, host: &'a str) -> BoxFuture<'a, Result<Vec<IpAddr>, String>>;
    fn send<'a>(&'a self, req: Pinned<'a>) -> BoxFuture<'a, Result<RawResponse, SendError>>;
}

/// Lista de domínios aprovados (vem de `network.hosts`, já validada pelo manifesto).
#[derive(Debug, Clone, Default)]
pub struct NetPolicy {
    hosts: Vec<String>,
}

impl NetPolicy {
    pub fn new(hosts: &[String]) -> Self {
        Self {
            hosts: hosts.iter().map(|h| h.to_ascii_lowercase()).collect(),
        }
    }

    pub fn host_allowed(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.hosts.iter().any(|rule| match rule.strip_prefix("*.") {
            // Curinga de um nível: `*.x.com` casa `a.x.com`, não `x.com` nem `a.b.x.com`.
            Some(suffix) => host
                .strip_suffix(suffix)
                .and_then(|p| p.strip_suffix('.'))
                .is_some_and(|label| !label.is_empty() && !label.contains('.')),
            None => *rule == host,
        })
    }
}

/// Limite de chamadas por janela de tempo (por app, compartilhado entre execuções).
pub struct RateLimiter {
    window: Duration,
    max: usize,
    hits: Mutex<std::collections::VecDeque<Instant>>,
}

impl RateLimiter {
    pub fn new(window: Duration, max: usize) -> Self {
        Self {
            window,
            max,
            hits: Mutex::new(Default::default()),
        }
    }

    /// Registra uma chamada; `false` se passou do teto da janela.
    pub fn allow(&self) -> bool {
        let now = Instant::now();
        let mut h = self.hits.lock().expect("rate");
        while h
            .front()
            .is_some_and(|t| now.duration_since(*t) > self.window)
        {
            h.pop_front();
        }
        if h.len() >= self.max {
            return false;
        }
        h.push_back(now);
        true
    }
}

/// Tudo que a rede de um mini-app precisa: política, transporte e limite de taxa.
#[derive(Clone)]
pub struct NetAccess {
    pub policy: NetPolicy,
    pub backend: Arc<dyn NetBackend>,
    pub rate: Arc<RateLimiter>,
}

impl NetAccess {
    pub fn new(policy: NetPolicy, backend: Arc<dyn NetBackend>) -> Self {
        Self {
            policy,
            backend,
            rate: Arc::new(RateLimiter::new(
                Duration::from_secs(60),
                MAX_CALLS_PER_MINUTE,
            )),
        }
    }
}

// ---------------------------------------------------------------- IP

/// IPv4 público? (tudo o que é local, privado, reservado ou de documentação é barrado)
pub fn ipv4_public(a: Ipv4Addr) -> bool {
    let o = a.octets();
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_private()
        || a.is_link_local()
        || a.is_broadcast()
        || a.is_multicast()
        || o[0] == 0
        || (o[0] == 100 && (o[1] & 0xC0) == 64) // 100.64.0.0/10 (CGNAT)
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24
        || (o[0] == 192 && o[1] == 0 && o[2] == 2) // TEST-NET-1
        || (o[0] == 198 && (o[1] & 0xFE) == 18) // 198.18.0.0/15 (benchmark)
        || (o[0] == 198 && o[1] == 51 && o[2] == 100) // TEST-NET-2
        || (o[0] == 203 && o[1] == 0 && o[2] == 113) // TEST-NET-3
        || o[0] >= 240) // reservado/futuro
}

/// IPv6 público? Endereços que embutem um IPv4 (mapeado, NAT64, 6to4) valem pelo IPv4.
pub fn ipv6_public(a: Ipv6Addr) -> bool {
    let s = a.segments();
    if a.is_unspecified() || a.is_loopback() || a.is_multicast() {
        return false;
    }
    if (s[0] & 0xFE00) == 0xFC00 || (s[0] & 0xFFC0) == 0xFE80 {
        return false; // ULA fc00::/7 e link-local fe80::/10
    }
    if s[0] == 0x2001 && s[1] == 0x0DB8 {
        return false; // documentação
    }
    let v4 = |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
    if s[..5] == [0, 0, 0, 0, 0] && s[5] == 0xFFFF {
        return ipv4_public(v4(s[6], s[7])); // ::ffff:a.b.c.d
    }
    if s[0] == 0x0064 && s[1] == 0xFF9B && s[2..6] == [0, 0, 0, 0] {
        return ipv4_public(v4(s[6], s[7])); // NAT64 64:ff9b::/96
    }
    if s[0] == 0x2002 {
        return ipv4_public(v4(s[1], s[2])); // 6to4
    }
    if s[..6] == [0, 0, 0, 0, 0, 0] {
        return false; // ::a.b.c.d (IPv4-compatible, obsoleto)
    }
    true
}

pub fn ip_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => ipv4_public(a),
        IpAddr::V6(a) => ipv6_public(a),
    }
}

// ---------------------------------------------------------------- URL e pedido

/// Interpreta e valida uma URL do guest. Devolve só `https` com domínio (nunca IP).
pub fn parse_url(raw: &str) -> Result<Url, NetError> {
    if raw.len() > MAX_URL_LEN || raw.chars().any(char::is_control) {
        return Err(NetError::BadUrl("tamanho ou caracteres inválidos"));
    }
    let url = Url::parse(raw).map_err(|_| NetError::BadUrl("não é uma URL"))?;
    check_url(&url)?;
    Ok(url)
}

fn check_url(url: &Url) -> Result<(), NetError> {
    if url.scheme() != "https" {
        return Err(NetError::BadUrl("só https"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(NetError::BadUrl("sem usuário ou senha na URL"));
    }
    if url.port().is_some_and(|p| p != 443) {
        return Err(NetError::BadUrl("só a porta 443"));
    }
    match url.host() {
        Some(Host::Domain(d)) if !d.is_empty() => Ok(()),
        Some(_) => Err(NetError::BadUrl("IP literal não é permitido")),
        None => Err(NetError::BadUrl("sem domínio")),
    }
}

fn parse_method(m: &str) -> Result<Method, NetError> {
    match m.to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::Get),
        "POST" => Ok(Method::Post),
        _ => Err(NetError::Method),
    }
}

/// Só `accept` e `content-type`, sem repetir, com valor curto e sem controle.
fn check_headers(h: &[(String, String)]) -> Result<Vec<(String, String)>, NetError> {
    if h.len() > MAX_HEADERS {
        return Err(NetError::Header);
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for (k, v) in h {
        let k = k.to_ascii_lowercase();
        if !(k == "accept" || k == "content-type")
            || out.iter().any(|(x, _)| *x == k)
            || v.len() > MAX_HEADER_VALUE
            || v.chars().any(char::is_control)
        {
            return Err(NetError::Header);
        }
        out.push((k, v.clone()));
    }
    Ok(out)
}

fn content_type_allowed(ct: &str) -> bool {
    let base = ct
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    base.starts_with("text/")
        || base == "application/json"
        || base == "application/xml"
        || base.ends_with("+json")
        || base.ends_with("+xml")
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// Host (sem query) para auditoria; `-` se a URL não passa nem na análise.
pub fn audit_host(raw: &str) -> String {
    Url::parse(raw)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.chars().take(100).collect()))
        .unwrap_or_else(|| "-".into())
}

/// Valida o pedido e o executa com a política, seguindo redirecionamentos com revalidação.
/// Não aplica o relógio da chamada nem os limites de taxa/execução (ficam com o chamador).
pub async fn fetch(
    policy: &NetPolicy,
    backend: &dyn NetBackend,
    req: HttpRequest,
) -> Result<HttpResponse, NetError> {
    let mut method = parse_method(&req.method)?;
    let headers = check_headers(&req.headers)?;
    if req.body.len() > MAX_REQUEST_BODY || (method == Method::Get && !req.body.is_empty()) {
        return Err(NetError::RequestTooLarge);
    }
    let mut url = parse_url(&req.url)?;
    let mut body = req.body;
    for hop in 0..=MAX_REDIRECTS {
        let host = url.host_str().unwrap_or("").to_string();
        if !policy.host_allowed(&host) {
            return Err(NetError::HostNotAllowed);
        }
        let ips = backend.resolve(&host).await.map_err(NetError::Transport)?;
        if ips.is_empty() {
            return Err(NetError::Transport("domínio sem endereço".into()));
        }
        // Basta um IP não público para recusar: não se escolhe "o bom" entre vários.
        if !ips.iter().all(|ip| ip_public(*ip)) {
            return Err(NetError::IpBlocked);
        }
        let raw = backend
            .send(Pinned {
                url: &url,
                ip: ips[0],
                method,
                headers: &headers,
                body: &body,
                max_response: MAX_RESPONSE_BODY,
                timeout: CALL_TIMEOUT,
            })
            .await
            .map_err(|e| match e {
                SendError::TooLarge => NetError::ResponseTooLarge,
                SendError::Timeout => NetError::Timeout,
                SendError::Other(m) => NetError::Transport(m),
            })?;
        if is_redirect(raw.status) {
            let Some(loc) = raw.location else {
                return Err(NetError::Transport("redirecionamento sem destino".into()));
            };
            if hop == MAX_REDIRECTS {
                return Err(NetError::TooManyRedirects);
            }
            let next = url
                .join(&loc)
                .map_err(|_| NetError::BadUrl("destino de redirecionamento inválido"))?;
            check_url(&next)?;
            if matches!(raw.status, 301..=303) {
                method = Method::Get;
                body = Vec::new();
            }
            url = next;
            continue;
        }
        if raw.body.len() > MAX_RESPONSE_BODY {
            return Err(NetError::ResponseTooLarge);
        }
        if !raw.body.is_empty() && !content_type_allowed(&raw.content_type) {
            return Err(NetError::ContentType);
        }
        return Ok(HttpResponse {
            status: raw.status,
            content_type: raw.content_type,
            body: raw.body,
        });
    }
    Err(NetError::TooManyRedirects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Backend falso: DNS e respostas fixos; guarda o que foi enviado.
    #[derive(Default)]
    struct Fake {
        dns: HashMap<String, Vec<IpAddr>>,
        pages: HashMap<String, (u16, String, Option<String>, Vec<u8>)>,
        sent: Mutex<Vec<(String, IpAddr, Method, usize)>>,
    }

    impl Fake {
        fn page(
            mut self,
            url: &str,
            status: u16,
            ct: &str,
            loc: Option<&str>,
            body: &[u8],
        ) -> Self {
            self.pages.insert(
                url.into(),
                (status, ct.into(), loc.map(Into::into), body.to_vec()),
            );
            self
        }
        fn dns(mut self, host: &str, ips: &[&str]) -> Self {
            self.dns.insert(
                host.into(),
                ips.iter().map(|i| i.parse().unwrap()).collect(),
            );
            self
        }
    }

    impl NetBackend for Fake {
        fn resolve<'a>(&'a self, host: &'a str) -> BoxFuture<'a, Result<Vec<IpAddr>, String>> {
            Box::pin(async move {
                Ok(self
                    .dns
                    .get(host)
                    .cloned()
                    .unwrap_or_else(|| vec!["93.184.216.34".parse().unwrap()]))
            })
        }
        fn send<'a>(&'a self, r: Pinned<'a>) -> BoxFuture<'a, Result<RawResponse, SendError>> {
            Box::pin(async move {
                self.sent.lock().unwrap().push((
                    r.url.as_str().into(),
                    r.ip,
                    r.method,
                    r.body.len(),
                ));
                match self.pages.get(r.url.as_str()) {
                    Some((status, ct, loc, body)) => {
                        if body.len() > r.max_response {
                            return Err(SendError::TooLarge);
                        }
                        Ok(RawResponse {
                            status: *status,
                            content_type: ct.clone(),
                            location: loc.clone(),
                            body: body.clone(),
                        })
                    }
                    None => Err(SendError::Other("404 simulado".into())),
                }
            })
        }
    }

    fn policy() -> NetPolicy {
        NetPolicy::new(&["api.exemplo.com".into(), "*.reddit.com".into()])
    }

    fn get(url: &str) -> HttpRequest {
        HttpRequest {
            method: "GET".into(),
            url: url.into(),
            headers: vec![],
            body: vec![],
        }
    }

    async fn run(f: &Fake, r: HttpRequest) -> Result<HttpResponse, NetError> {
        fetch(&policy(), f, r).await
    }

    #[tokio::test]
    async fn allowed_request_goes_to_the_pinned_validated_ip() {
        let f = Fake::default().page(
            "https://api.exemplo.com/v1?x=1",
            200,
            "application/json; charset=utf-8",
            None,
            b"{}",
        );
        let r = run(&f, get("https://API.exemplo.com/v1?x=1"))
            .await
            .unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, b"{}".as_slice()));
        let sent = f.sent.lock().unwrap();
        assert_eq!(sent[0].1, "93.184.216.34".parse::<IpAddr>().unwrap());
    }

    #[tokio::test]
    async fn urls_that_are_not_plain_https_domains_are_refused() {
        let f = Fake::default();
        for bad in [
            "http://api.exemplo.com/",
            "ftp://api.exemplo.com/",
            "file:///etc/passwd",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://2130706433/",
            "https://0x7f.1/",
            "https://127.1/",
            "https://api.exemplo.com:8443/",
            "https://user:pw@api.exemplo.com/",
            "https://user@api.exemplo.com/",
            "api.exemplo.com",
            "",
            "https://",
            "//api.exemplo.com/",
            "javascript:alert(1)",
        ] {
            let e = run(&f, get(bad)).await.unwrap_err();
            assert!(matches!(e, NetError::BadUrl(_)), "{bad:?} -> {e:?}");
        }
        let longa = format!("https://api.exemplo.com/{}", "a".repeat(MAX_URL_LEN));
        assert!(matches!(
            run(&f, get(&longa)).await,
            Err(NetError::BadUrl(_))
        ));
        assert!(matches!(
            run(&f, get("https://api.exemplo.com/\u{0}")).await,
            Err(NetError::BadUrl(_))
        ));
        assert!(f.sent.lock().unwrap().is_empty(), "nada saiu");
    }

    #[tokio::test]
    async fn hosts_outside_the_list_are_refused_and_wildcard_is_one_level() {
        let f = Fake::default().page("https://old.reddit.com/", 200, "text/html", None, b"ok");
        assert!(run(&f, get("https://old.reddit.com/")).await.is_ok());
        for bad in [
            "https://reddit.com/",
            "https://a.b.reddit.com/",
            "https://evilreddit.com/",
            "https://reddit.com.evil.com/",
            "https://exemplo.com/",
            "https://api.exemplo.com.evil.com/",
        ] {
            assert_eq!(
                run(&f, get(bad)).await,
                Err(NetError::HostNotAllowed),
                "{bad}"
            );
        }
    }

    #[tokio::test]
    async fn any_non_public_ip_after_dns_blocks_the_call() {
        let page = |f: Fake| f.page("https://api.exemplo.com/", 200, "text/plain", None, b"x");
        for ip in [
            "127.0.0.1",
            "10.0.0.5",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "198.18.0.1",
            "192.0.2.1",
            "240.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::7f00:1",
            "2002:7f00:1::1",
            "2001:db8::1",
            "::1.2.3.4",
        ] {
            let f = page(Fake::default().dns("api.exemplo.com", &[ip]));
            assert_eq!(
                run(&f, get("https://api.exemplo.com/")).await,
                Err(NetError::IpBlocked),
                "{ip}"
            );
            assert!(f.sent.lock().unwrap().is_empty(), "{ip}: nada saiu");
        }
        // Um público junto com um privado também recusa (sem escolher "o bom").
        let f = page(Fake::default().dns("api.exemplo.com", &["93.184.216.34", "10.0.0.1"]));
        assert_eq!(
            run(&f, get("https://api.exemplo.com/")).await,
            Err(NetError::IpBlocked)
        );
        // E públicos de verdade passam.
        for ip in [
            "93.184.216.34",
            "8.8.8.8",
            "2606:2800:220:1:248:1893:25c8:1946",
            "::ffff:8.8.8.8",
        ] {
            let f = page(Fake::default().dns("api.exemplo.com", &[ip]));
            assert!(
                run(&f, get("https://api.exemplo.com/")).await.is_ok(),
                "{ip}"
            );
        }
    }

    #[tokio::test]
    async fn redirects_are_revalidated_and_capped() {
        // Para fora da lista, para http e para IP literal: tudo recusado.
        for destino in [
            "https://evil.com/",
            "http://api.exemplo.com/x",
            "https://10.0.0.1/",
            "https://api.exemplo.com:9999/",
        ] {
            let f = Fake::default().page(
                "https://api.exemplo.com/",
                302,
                "text/plain",
                Some(destino),
                b"",
            );
            assert!(
                run(&f, get("https://api.exemplo.com/")).await.is_err(),
                "{destino}"
            );
            assert_eq!(
                f.sent.lock().unwrap().len(),
                1,
                "o destino nunca foi contatado: {destino}"
            );
        }
        // Redirecionamento relativo dentro da lista funciona; 302 de POST vira GET sem corpo.
        let f = Fake::default()
            .page(
                "https://api.exemplo.com/a",
                302,
                "text/plain",
                Some("/b"),
                b"",
            )
            .page("https://api.exemplo.com/b", 200, "text/plain", None, b"fim");
        let mut r = get("https://api.exemplo.com/a");
        r.method = "post".into();
        r.body = b"dados".to_vec();
        assert_eq!(run(&f, r).await.unwrap().body, b"fim");
        {
            let sent = f.sent.lock().unwrap();
            assert_eq!((sent[0].2, sent[0].3), (Method::Post, 5));
            assert_eq!((sent[1].2, sent[1].3), (Method::Get, 0));
        }
        // Laço de redirecionamentos.
        let f = Fake::default().page(
            "https://api.exemplo.com/",
            301,
            "text/plain",
            Some("/"),
            b"",
        );
        assert_eq!(
            run(&f, get("https://api.exemplo.com/")).await,
            Err(NetError::TooManyRedirects)
        );
        assert_eq!(f.sent.lock().unwrap().len(), MAX_REDIRECTS + 1);
    }

    #[tokio::test]
    async fn method_headers_and_bodies_are_limited() {
        let f = Fake::default().page("https://api.exemplo.com/", 200, "text/plain", None, b"x");
        let mut r = get("https://api.exemplo.com/");
        r.method = "DELETE".into();
        assert_eq!(run(&f, r).await, Err(NetError::Method));
        for h in [
            ("Authorization", "Bearer x"),
            ("Cookie", "a=b"),
            ("Host", "evil.com"),
            ("Referer", "x"),
            ("Accept", "a\nb"),
        ] {
            let mut r = get("https://api.exemplo.com/");
            r.headers = vec![(h.0.into(), h.1.into())];
            assert_eq!(run(&f, r).await, Err(NetError::Header), "{h:?}");
        }
        let mut r = get("https://api.exemplo.com/");
        r.headers = vec![("Accept".into(), "a".into()), ("accept".into(), "b".into())];
        assert_eq!(run(&f, r).await, Err(NetError::Header), "repetido");
        let mut r = get("https://api.exemplo.com/");
        r.headers = vec![("Accept".into(), "application/json".into())];
        assert!(run(&f, r).await.is_ok());
        let mut r = get("https://api.exemplo.com/");
        r.body = b"x".to_vec(); // GET com corpo
        assert_eq!(run(&f, r).await, Err(NetError::RequestTooLarge));
        let mut r = get("https://api.exemplo.com/");
        r.method = "POST".into();
        r.body = vec![0; MAX_REQUEST_BODY + 1];
        assert_eq!(run(&f, r).await, Err(NetError::RequestTooLarge));
    }

    #[tokio::test]
    async fn responses_are_capped_and_only_text_like() {
        let grande = vec![b'a'; MAX_RESPONSE_BODY + 1];
        let f = Fake::default()
            .page(
                "https://api.exemplo.com/big",
                200,
                "text/plain",
                None,
                &grande,
            )
            .page(
                "https://api.exemplo.com/bin",
                200,
                "application/octet-stream",
                None,
                b"\x00\x01",
            )
            .page("https://api.exemplo.com/img", 200, "image/png", None, b"x")
            .page(
                "https://api.exemplo.com/ld",
                200,
                "application/ld+json",
                None,
                b"{}",
            )
            .page("https://api.exemplo.com/vazio", 204, "", None, b"");
        assert_eq!(
            run(&f, get("https://api.exemplo.com/big")).await,
            Err(NetError::ResponseTooLarge)
        );
        assert_eq!(
            run(&f, get("https://api.exemplo.com/bin")).await,
            Err(NetError::ContentType)
        );
        assert_eq!(
            run(&f, get("https://api.exemplo.com/img")).await,
            Err(NetError::ContentType)
        );
        assert!(run(&f, get("https://api.exemplo.com/ld")).await.is_ok());
        assert_eq!(
            run(&f, get("https://api.exemplo.com/vazio"))
                .await
                .unwrap()
                .status,
            204
        );
        // Falha do servidor vira erro de transporte (não é "regra").
        let e = run(&f, get("https://api.exemplo.com/nada"))
            .await
            .unwrap_err();
        assert!(matches!(e, NetError::Transport(_)) && !e.is_rule());
    }

    #[test]
    fn rate_limiter_caps_a_window() {
        let r = RateLimiter::new(Duration::from_secs(60), 3);
        assert!(r.allow() && r.allow() && r.allow());
        assert!(!r.allow());
        let curto = RateLimiter::new(Duration::from_millis(20), 1);
        assert!(curto.allow() && !curto.allow());
        std::thread::sleep(Duration::from_millis(40));
        assert!(curto.allow(), "a janela andou");
    }

    #[test]
    fn arbitrary_inputs_never_panic() {
        // Mutação simples sobre URLs e IPs: só importa não entrar em pânico.
        let base = "https://user:pw@[::ffff:127.0.0.1]:443/a b?c=d#e";
        let mut s = base.to_string();
        for i in 0..base.len() {
            for ch in ['\u{0}', '/', '@', ':', '[', ']', '%', '\u{202e}', ' '] {
                let mut t: Vec<char> = base.chars().collect();
                let n = t.len();
                t[i % n] = ch;
                s = t.into_iter().collect();
                let _ = parse_url(&s);
                let _ = audit_host(&s);
                let _ = NetPolicy::new(&[s.clone()]).host_allowed(&s);
            }
        }
        let _ = s;
        for a in 0..=255u8 {
            for b in [0u8, 1, 64, 127, 168, 254, 255] {
                let _ = ipv4_public(Ipv4Addr::new(a, b, b, a));
            }
        }
    }
}
