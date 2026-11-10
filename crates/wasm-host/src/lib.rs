//! Host de mini-apps WebAssembly (Wasmtime + Component Model, ADR-004).
//!
//! Sem dependência de Tauri, como o `superapp-core`. Garantias:
//! - **fuel sempre ligado**: a engine é criada aqui, nunca recebida de fora;
//! - **permissão = interface WIT ligada**: só o que o manifesto concede entra no linker,
//!   e importar o resto faz o carregamento falhar;
//! - **limites por execução**: fuel, memória, tempo total e yield assíncrono (o host
//!   continua responsivo durante um laço do guest);
//! - **toda chamada do guest ao host é auditada** com a identidade `app:<id>`.

pub mod lifecycle;
pub mod storage;
pub mod ui;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use superapp_core::{AuditEvent, AuditSink, Decision};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, ResourceLimiter, Store, Trap};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

pub use storage::{DirStorage, MemoryStorage, Storage};
use superapp_package::{manifest, Permission, VerifiedPackage};

wasmtime::component::bindgen!({
    world: "miniapp",
    path: "wit",
    imports: { default: async },
    exports: { default: async },
});

use superapp::host::{clock, log, storage as wit_storage};

/// Interface do host que o mini-app pode importar (cada uma é uma permissão).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Interface {
    Log,
    Storage,
    Clock,
}

impl Interface {
    fn wit_name(self) -> &'static str {
        match self {
            Interface::Log => "superapp:host/log",
            Interface::Storage => "superapp:host/storage",
            Interface::Clock => "superapp:host/clock",
        }
    }
}

/// Conjunto de interfaces concedidas (vem do manifesto aprovado pelo usuário).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grants(BTreeSet<Interface>);

impl Grants {
    pub fn none() -> Self {
        Self::default()
    }
    pub fn all() -> Self {
        Self::none()
            .with(Interface::Log)
            .with(Interface::Storage)
            .with(Interface::Clock)
    }
    #[must_use]
    pub fn with(mut self, i: Interface) -> Self {
        self.0.insert(i);
        self
    }
    pub fn has(&self, i: Interface) -> bool {
        self.0.contains(&i)
    }
    /// Todas as interfaces deste conjunto estão em `other`?
    pub fn is_subset_of(&self, other: &Grants) -> bool {
        self.0.is_subset(&other.0)
    }
    /// Concessão correspondente às permissões pedidas num manifesto.
    pub fn from_permissions(perms: &[Permission]) -> Self {
        perms.iter().fold(Self::none(), |g, p| {
            g.with(match p {
                Permission::Log => Interface::Log,
                Permission::Storage => Interface::Storage,
                Permission::Clock => Interface::Clock,
            })
        })
    }
}

/// Limites de uma execução. Os padrões são conservadores.
#[derive(Debug, Clone)]
pub struct Limits {
    pub fuel: u64,
    pub memory_bytes: usize,
    pub timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            fuel: 50_000_000,
            memory_bytes: 16 * 1024 * 1024,
            timeout: Duration::from_secs(5),
        }
    }
}

impl Limits {
    /// Limites pedidos pelo manifesto sobre os padrões. O manifesto já foi validado contra
    /// os tetos do host; o `min` é só um cinto de segurança.
    pub fn from_manifest(m: &superapp_package::Manifest) -> Self {
        let mut l = Self::default();
        if let Some(r) = &m.limits {
            if let Some(f) = r.fuel {
                l.fuel = f.min(manifest::MAX_FUEL);
            }
            if let Some(mb) = r.memory_mb {
                l.memory_bytes = mb.min(manifest::MAX_MEMORY_MB) as usize * 1024 * 1024;
            }
            if let Some(ms) = r.timeout_ms {
                l.timeout = Duration::from_millis(u64::from(ms.min(manifest::MAX_TIMEOUT_MS)));
            }
        }
        l
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WasmError {
    #[error("componente inválido: {0}")]
    Load(String),
    #[error("o mini-app importa `{0}`, que não foi concedida")]
    ImportNotGranted(String),
    #[error("combustível esgotado (laço ou computação excessiva)")]
    OutOfFuel,
    #[error("limite de memória excedido ({limit_bytes} bytes)")]
    OutOfMemory { limit_bytes: usize },
    #[error("tempo limite excedido")]
    Timeout,
    #[error("o mini-app falhou: {0}")]
    Trap(String),
    #[error("erro do mini-app: {0}")]
    App(String),
}

/// Resultado de uma execução bem-sucedida.
#[derive(Debug)]
pub struct RunOutput {
    pub output: String,
    pub logs: Vec<String>,
    pub fuel_used: u64,
}

const MAX_LOG_LINES: usize = 200;
const MAX_LOG_LINE_BYTES: usize = 1024;
const MAX_TABLE_ELEMENTS: usize = 10_000;

/// Limitador de recursos: nega crescimento acima do teto e lembra que negou, para o erro
/// poder ser traduzido (o guest só vê `unreachable` do alocador, ver ADR-004).
struct Limiter {
    max_memory: usize,
    memory_denied: bool,
}

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.max_memory {
            self.memory_denied = true;
            return Ok(false);
        }
        Ok(true)
    }
    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= MAX_TABLE_ELEMENTS)
    }
}

struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limiter: Limiter,
    app: String,
    storage: Arc<dyn Storage>,
    audit: Arc<dyn AuditSink>,
    logs: Vec<String>,
}

impl State {
    fn audited(&self, command: &str) {
        self.audit.record(AuditEvent {
            window: format!("app:{}", self.app),
            command: command.to_owned(),
            decision: Decision::Allowed,
        });
    }
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl log::Host for State {
    async fn log(&mut self, msg: String) {
        self.audited("log.log");
        if self.logs.len() < MAX_LOG_LINES {
            let mut end = msg.len().min(MAX_LOG_LINE_BYTES);
            while !msg.is_char_boundary(end) {
                end -= 1;
            }
            self.logs.push(msg[..end].to_owned());
        }
    }
}

impl wit_storage::Host for State {
    async fn get(&mut self, key: String) -> Option<Vec<u8>> {
        self.audited("storage.get");
        self.storage.get(&key).ok().flatten()
    }
    async fn set(&mut self, key: String, value: Vec<u8>) -> Result<(), String> {
        self.audited("storage.set");
        self.storage.set(&key, &value)
    }
    async fn delete(&mut self, key: String) -> Result<(), String> {
        self.audited("storage.delete");
        self.storage.delete(&key)
    }
}

impl clock::Host for State {
    async fn now_ms(&mut self) -> u64 {
        self.audited("clock.now_ms");
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// Runtime compartilhado: uma engine com fuel obrigatório e o destino da auditoria.
pub struct Runtime {
    engine: Engine,
    audit: Arc<dyn AuditSink>,
}

impl Runtime {
    pub fn new(audit: Arc<dyn AuditSink>) -> Result<Self, WasmError> {
        let mut cfg = Config::new();
        cfg.consume_fuel(true); // obrigatório: sem isso um laço do guest nunca é interrompido
        let engine = Engine::new(&cfg).map_err(|e| WasmError::Load(e.to_string()))?;
        Ok(Self { engine, audit })
    }

    /// Carrega um pacote já verificado. `approved` é o que o usuário consentiu e nunca pode
    /// passar do que o manifesto pediu. Como só o aprovado é ligado, um código que importe
    /// mais do que o manifesto declarou falha em `ImportNotGranted`.
    pub fn load_verified(
        &self,
        pkg: &VerifiedPackage,
        approved: &Grants,
        storage: Arc<dyn Storage>,
    ) -> Result<MiniApp, WasmError> {
        let requested = Grants::from_permissions(&pkg.manifest.permissions);
        if !approved.is_subset_of(&requested) {
            return Err(WasmError::Load(
                "concessão maior que a pedida no manifesto".into(),
            ));
        }
        self.load(&pkg.manifest.id, &pkg.wasm, approved, storage)
    }

    /// Compila o componente e liga só as interfaces concedidas. Falha se o componente
    /// importar algo que não foi concedido.
    pub fn load(
        &self,
        app_id: &str,
        wasm: &[u8],
        grants: &Grants,
        storage: Arc<dyn Storage>,
    ) -> Result<MiniApp, WasmError> {
        if !storage::valid_app_id(app_id) {
            return Err(WasmError::Load("id de mini-app inválido".into()));
        }
        let component =
            Component::new(&self.engine, wasm).map_err(|e| WasmError::Load(first_line(&e)))?;
        let mut linker: Linker<State> = Linker::new(&self.engine);
        let load_err = |e: wasmtime::Error| WasmError::Load(first_line(&e));
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(load_err)?;
        if grants.has(Interface::Log) {
            log::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s).map_err(load_err)?;
        }
        if grants.has(Interface::Storage) {
            wit_storage::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(load_err)?;
        }
        if grants.has(Interface::Clock) {
            clock::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s).map_err(load_err)?;
        }
        let instance_pre = linker.instantiate_pre(&component).map_err(|e| {
            let text = format!("{e:#}");
            for i in [Interface::Log, Interface::Storage, Interface::Clock] {
                if !grants.has(i) && text.contains(i.wit_name()) {
                    self.audit.record(AuditEvent {
                        window: format!("app:{app_id}"),
                        command: format!("load:{}", i.wit_name()),
                        decision: Decision::DeniedNoCapability,
                    });
                    return WasmError::ImportNotGranted(i.wit_name().to_owned());
                }
            }
            WasmError::Load(first_line(&e))
        })?;
        let pre = MiniappPre::new(instance_pre).map_err(load_err)?;
        Ok(MiniApp {
            engine: self.engine.clone(),
            audit: self.audit.clone(),
            id: app_id.to_owned(),
            pre,
            storage,
        })
    }
}

fn first_line(e: &wasmtime::Error) -> String {
    format!("{e:#}").lines().next().unwrap_or("").to_owned()
}

/// Mini-app carregado (componente já ligado e verificado). Cada `run` cria um `Store` novo:
/// nenhum estado vaza entre execuções, exceto o que o app grava em `storage`.
pub struct MiniApp {
    engine: Engine,
    audit: Arc<dyn AuditSink>,
    id: String,
    pre: MiniappPre<State>,
    storage: Arc<dyn Storage>,
}

impl MiniApp {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn run(&self, input: &str, limits: &Limits) -> Result<RunOutput, WasmError> {
        // Sem preopens, sem env, sem rede: nenhuma permissão WASI concedida.
        let state = State {
            wasi: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
            limiter: Limiter {
                max_memory: limits.memory_bytes,
                memory_denied: false,
            },
            app: self.id.clone(),
            storage: self.storage.clone(),
            audit: self.audit.clone(),
            logs: Vec::new(),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limiter);
        store
            .set_fuel(limits.fuel)
            .map_err(|e| WasmError::Load(first_line(&e)))?;
        // Devolve o controle ao executor a cada 10 mil unidades: o host segue responsivo e
        // o timeout consegue interromper um guest que só calcula.
        store
            .fuel_async_yield_interval(Some(10_000))
            .map_err(|e| WasmError::Load(first_line(&e)))?;

        let result = {
            let fut = async {
                let app = self.pre.instantiate_async(&mut store).await?;
                app.call_run(&mut store, input).await
            };
            tokio::time::timeout(limits.timeout, fut).await
        };

        let fuel_used = limits.fuel.saturating_sub(store.get_fuel().unwrap_or(0));
        match result {
            Err(_elapsed) => Err(WasmError::Timeout),
            Ok(Err(e)) => Err(classify(&e, &store, limits)),
            Ok(Ok(Err(app_err))) => Err(WasmError::App(app_err)),
            Ok(Ok(Ok(output))) => Ok(RunOutput {
                output,
                logs: std::mem::take(&mut store.data_mut().logs),
                fuel_used,
            }),
        }
    }
}

fn classify(e: &wasmtime::Error, store: &Store<State>, limits: &Limits) -> WasmError {
    if e.downcast_ref::<Trap>() == Some(&Trap::OutOfFuel) {
        return WasmError::OutOfFuel;
    }
    if store.data().limiter.memory_denied {
        return WasmError::OutOfMemory {
            limit_bytes: limits.memory_bytes,
        };
    }
    WasmError::Trap(first_line(e))
}
