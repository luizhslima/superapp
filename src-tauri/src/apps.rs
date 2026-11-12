//! Integração do gerenciador de mini-apps com o Router do core.
//!
//! O arquivo `.sapp` nunca é escolhido pela UI: chega por arrastar-e-soltar, tratado aqui no
//! Rust (a UI não informa caminhos, então não consegue pedir para ler arquivos arbitrários).
//! O pacote fica "pendente" até a UI mostrar o consentimento e chamar `apps.install`.
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use superapp_core::{AuditSink, ErrorCode, IpcError, Router};
use superapp_package::{Permission, TrustStore};
use superapp_wasm_host::lifecycle::{LifecycleError, Preview, Registry};
use superapp_wasm_host::ui::Event;
use superapp_wasm_host::WasmError;

/// Maior arquivo aceito no arrastar-e-soltar (teto do wasm + manifesto + cabeçalho).
const MAX_FILE_BYTES: u64 = 33 * 1024 * 1024;
/// Quantos pacotes podem esperar consentimento ao mesmo tempo.
const MAX_PENDING: usize = 4;

struct Pending {
    token: String,
    bytes: Vec<u8>,
    preview: Preview,
}

pub struct AppsState {
    registry: Registry,
    trust: TrustStore,
    data_dir: PathBuf,
    pending: Mutex<Vec<Pending>>,
}

/// Raiz de dados: `SUPERAPP_DATA_DIR` (testes) ou o diretório de dados do app.
pub fn open(data_dir: PathBuf, audit: Arc<dyn AuditSink>) -> Result<Arc<AppsState>, String> {
    // Chaves de autores confiáveis: <dados>/trusted-keys.txt (uma chave pública hex por linha).
    // Sem o arquivo, ninguém é confiável e nada pode ser instalado.
    let trust = match std::fs::read_to_string(data_dir.join("trusted-keys.txt")) {
        Ok(t) => TrustStore::from_hex_lines(&t)?,
        Err(_) => TrustStore::default(),
    };
    let registry =
        Registry::open(data_dir.clone(), trust.clone(), audit).map_err(|e| e.to_string())?;
    Ok(Arc::new(AppsState {
        registry,
        trust,
        data_dir,
        pending: Mutex::new(Vec::new()),
    }))
}

impl AppsState {
    /// Chaves de autores confiáveis (a mesma lista vale para pacotes e para o catálogo).
    pub fn trust(&self) -> &TrustStore {
        &self.trust
    }

    /// Raiz de dados do app (onde ficam `trusted-keys.txt`, `apps/` e `data/`).
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Resumo para o relatório de diagnóstico: só id, versão e estado (sem dados nem caminhos).
    pub fn summary(&self) -> Vec<Value> {
        self.registry
            .list()
            .iter()
            .map(|a| json!({ "id": a.id, "version": a.version, "enabled": a.enabled, "problem": a.problem.is_some() }))
            .collect()
    }

    /// Lê um `.sapp` do disco, verifica e deixa pendente de consentimento.
    pub fn stage_path(&self, path: &Path) -> Result<(), String> {
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            return Err("arquivo inválido ou grande demais".into());
        }
        if path.extension().and_then(|e| e.to_str()) != Some("sapp") {
            return Err("esperado um arquivo .sapp".into());
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        self.stage_bytes(bytes)
    }

    /// Verifica um `.sapp` já em memória e o deixa pendente de consentimento. Vale para
    /// arquivo arrastado e para download do catálogo: o caminho de verificação é o mesmo.
    pub fn stage_bytes(&self, bytes: Vec<u8>) -> Result<(), String> {
        let preview = self.registry.inspect(&bytes).map_err(|e| e.to_string())?;
        let token = superapp_package::sha256_hex(&bytes);
        let mut p = self.pending.lock().expect("pending");
        p.retain(|x| x.token != token);
        if p.len() >= MAX_PENDING {
            p.remove(0);
        }
        p.push(Pending {
            token,
            bytes,
            preview,
        });
        Ok(())
    }
}

fn map_err(e: LifecycleError) -> IpcError {
    let msg = e.to_string();
    match e {
        LifecycleError::NotInstalled => IpcError::new(ErrorCode::NotFound, msg),
        LifecycleError::Disabled
        | LifecycleError::Broken(_)
        | LifecycleError::Wasm(WasmError::ImportNotGranted(_)) => {
            IpcError::new(ErrorCode::PermissionDenied, msg)
        }
        LifecycleError::Package(_)
        | LifecycleError::Rollback { .. }
        | LifecycleError::ApprovalExceedsRequest
        | LifecycleError::InvalidUi(_) => IpcError::new(ErrorCode::InvalidArgs, msg),
        // Falhas do mini-app (memória, fuel, tempo, erro próprio) e de disco: a mensagem é
        // útil ao usuário e não contém segredos do host.
        LifecycleError::Wasm(_) | LifecycleError::Io(_) => IpcError::new(ErrorCode::Internal, msg),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallArgs {
    token: String,
    approved: Vec<Permission>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenArgs {
    token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnableArgs {
    id: String,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UninstallArgs {
    id: String,
    keep_data: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DispatchArgs {
    id: String,
    event: Event,
}

/// Capability única dos comandos de gerenciamento. Só a janela principal a recebe.
pub const CAP: &str = "apps:manage";

pub fn register(r: &mut Router, st: Arc<AppsState>) {
    let s = st.clone();
    r.register("apps.list", CAP, move |_c, _a: Value| Ok(s.registry.list()));

    let s = st.clone();
    r.register("apps.pending", CAP, move |_c, _a: Value| {
        let p = s.pending.lock().expect("pending");
        Ok(p.iter()
            .map(|x| json!({ "token": x.token, "preview": x.preview }))
            .collect::<Vec<_>>())
    });

    let s = st.clone();
    r.register("apps.install", CAP, move |_c, a: InstallArgs| {
        let mut p = s.pending.lock().expect("pending");
        let idx = p
            .iter()
            .position(|x| x.token == a.token)
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "pacote pendente não encontrado"))?;
        let installed = s
            .registry
            .install(&p[idx].bytes, &a.approved)
            .map_err(map_err)?;
        p.remove(idx);
        Ok(installed)
    });

    let s = st.clone();
    r.register("apps.discard", CAP, move |_c, a: TokenArgs| {
        s.pending
            .lock()
            .expect("pending")
            .retain(|x| x.token != a.token);
        Ok(Value::Null)
    });

    let s = st.clone();
    r.register("apps.set_enabled", CAP, move |_c, a: EnableArgs| {
        s.registry.set_enabled(&a.id, a.enabled).map_err(map_err)
    });

    let s = st.clone();
    r.register("apps.uninstall", CAP, move |_c, a: UninstallArgs| {
        s.registry.uninstall(&a.id, a.keep_data).map_err(map_err)?;
        Ok(Value::Null)
    });

    // Executa o mini-app (pode levar até o timeout do manifesto): o `ipc` roda este handler
    // numa thread de bloqueio, então a UI não congela.
    r.register("apps.dispatch", CAP, move |_c, a: DispatchArgs| {
        tauri::async_runtime::block_on(st.registry.dispatch(&a.id, &a.event)).map_err(map_err)
    });
}
