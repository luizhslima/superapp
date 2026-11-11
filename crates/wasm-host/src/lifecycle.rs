//! Ciclo de vida dos mini-apps: inspecionar, instalar, atualizar, suspender, remover e
//! despachar eventos de UI. Estado em disco:
//!
//! ```text
//! <raiz>/apps/<id>/package.sapp   pacote original (reverificado a cada carga)
//! <raiz>/apps/<id>/state.json     permissões aprovadas + habilitado
//! <raiz>/data/<id>/               storage do mini-app (sobrevive a atualizações)
//! ```
//! Regras: aprovado ⊆ pedido; versão nova > instalada (anti-rollback); o pacote é
//! reverificado contra a `TrustStore` atual em toda carga (chave removida = app para de rodar).
use crate::ui::{Event, Node};
use crate::{DirStorage, Grants, Limits, MiniApp, Runtime, WasmError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use superapp_core::AuditSink;
use superapp_package::{PackageError, Permission, TrustStore, VerifiedPackage};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("pacote recusado: {0}")]
    Package(#[from] PackageError),
    #[error("versão {new} não é maior que a instalada ({installed})")]
    Rollback { installed: String, new: String },
    #[error("a aprovação pede permissões que o manifesto não declarou")]
    ApprovalExceedsRequest,
    #[error("mini-app não instalado")]
    NotInstalled,
    #[error("mini-app suspenso")]
    Disabled,
    #[error("mini-app com problema: {0}")]
    Broken(String),
    #[error("{0}")]
    Wasm(#[from] WasmError),
    #[error("UI inválida: {0}")]
    InvalidUi(String),
    #[error("erro de armazenamento: {0}")]
    Io(String),
}

/// Mini-app instalado, como a UI o enxerga.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub requested: Vec<Permission>,
    pub approved: Vec<Permission>,
    pub enabled: bool,
    /// Preenchido quando o pacote em disco não passa mais na verificação.
    pub problem: Option<String>,
}

/// O que o usuário precisa ver antes de consentir.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Preview {
    pub id: String,
    pub name: String,
    pub version: String,
    pub requested: Vec<Permission>,
    pub is_update: bool,
    pub installed_version: Option<String>,
    pub previously_approved: Vec<Permission>,
    /// Permissões pedidas agora que o usuário ainda não tinha aprovado.
    pub added: Vec<Permission>,
    /// Chave pública do autor (hex).
    pub signer: String,
}

#[derive(Serialize, Deserialize)]
struct State {
    approved: Vec<Permission>,
    enabled: bool,
}

struct Loaded {
    app: Arc<MiniApp>,
    limits: Limits,
}

#[derive(Default)]
struct Inner {
    apps: HashMap<String, Installed>,
    loaded: HashMap<String, Loaded>,
}

pub struct Registry {
    root: PathBuf,
    runtime: Runtime,
    trust: TrustStore,
    inner: Mutex<Inner>,
}

fn io<E: std::fmt::Display>(e: E) -> LifecycleError {
    LifecycleError::Io(e.to_string())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), LifecycleError> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

fn version_tuple(v: &str) -> (u32, u32, u32) {
    let mut p = v.split('.').map(|x| x.parse::<u32>().unwrap_or(0));
    (
        p.next().unwrap_or(0),
        p.next().unwrap_or(0),
        p.next().unwrap_or(0),
    )
}

fn normalize(perms: &[Permission]) -> Vec<Permission> {
    let mut v = perms.to_vec();
    v.sort();
    v.dedup();
    v
}

impl Registry {
    pub fn open(
        root: impl Into<PathBuf>,
        trust: TrustStore,
        audit: Arc<dyn AuditSink>,
    ) -> Result<Self, LifecycleError> {
        let root = root.into();
        std::fs::create_dir_all(root.join("apps")).map_err(io)?;
        std::fs::create_dir_all(root.join("data")).map_err(io)?;
        let reg = Self {
            root,
            runtime: Runtime::new(audit)?,
            trust,
            inner: Mutex::new(Inner::default()),
        };
        let mut apps = HashMap::new();
        for entry in std::fs::read_dir(reg.root.join("apps"))
            .map_err(io)?
            .flatten()
        {
            let id = entry.file_name().to_string_lossy().into_owned();
            if superapp_package::valid_app_id(&id) {
                apps.insert(id.clone(), reg.read_entry(&id));
            }
        }
        reg.inner.lock().expect("registry").apps = apps;
        Ok(reg)
    }

    fn app_dir(&self, id: &str) -> PathBuf {
        self.root.join("apps").join(id)
    }

    fn verified_from_disk(&self, id: &str) -> Result<VerifiedPackage, LifecycleError> {
        let bytes = std::fs::read(self.app_dir(id).join("package.sapp")).map_err(io)?;
        let pkg = superapp_package::verify(&bytes, &self.trust)?;
        if pkg.manifest.id != id {
            return Err(LifecycleError::Broken(
                "id do pacote difere do diretório".into(),
            ));
        }
        Ok(pkg)
    }

    /// Lê um app do disco; se o pacote não passa na verificação, devolve a entrada marcada.
    fn read_entry(&self, id: &str) -> Installed {
        let state: Option<State> = std::fs::read(self.app_dir(id).join("state.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        match self.verified_from_disk(id) {
            Ok(pkg) => {
                let (approved, enabled) = match state {
                    Some(s) => (normalize(&s.approved), s.enabled),
                    None => (Vec::new(), false),
                };
                Installed {
                    id: id.into(),
                    name: pkg.manifest.name.clone(),
                    version: pkg.manifest.version.clone(),
                    requested: normalize(&pkg.manifest.permissions),
                    approved,
                    enabled,
                    problem: None,
                }
            }
            Err(e) => Installed {
                id: id.into(),
                name: id.into(),
                version: "?".into(),
                requested: Vec::new(),
                approved: Vec::new(),
                enabled: false,
                problem: Some(e.to_string()),
            },
        }
    }

    fn check_not_rollback(
        &self,
        pkg: &VerifiedPackage,
    ) -> Result<Option<Installed>, LifecycleError> {
        let g = self.inner.lock().expect("registry");
        let Some(cur) = g.apps.get(&pkg.manifest.id) else {
            return Ok(None);
        };
        if cur.problem.is_none()
            && version_tuple(&pkg.manifest.version) <= version_tuple(&cur.version)
        {
            return Err(LifecycleError::Rollback {
                installed: cur.version.clone(),
                new: pkg.manifest.version.clone(),
            });
        }
        Ok(Some(cur.clone()))
    }

    /// Verifica um pacote e descreve o que seria instalado, sem instalar nada.
    pub fn inspect(&self, bytes: &[u8]) -> Result<Preview, LifecycleError> {
        let pkg = superapp_package::verify(bytes, &self.trust)?;
        let current = self.check_not_rollback(&pkg)?;
        let requested = normalize(&pkg.manifest.permissions);
        let previously = current
            .as_ref()
            .filter(|c| c.problem.is_none())
            .map(|c| c.approved.clone())
            .unwrap_or_default();
        let added = requested
            .iter()
            .copied()
            .filter(|p| !previously.contains(p))
            .collect();
        Ok(Preview {
            id: pkg.manifest.id.clone(),
            name: pkg.manifest.name.clone(),
            version: pkg.manifest.version.clone(),
            requested,
            is_update: current.is_some(),
            installed_version: current.map(|c| c.version),
            previously_approved: previously,
            added,
            signer: superapp_package::to_hex(&pkg.signer),
        })
    }

    /// Instala ou atualiza. Reverifica tudo (não confia em `inspect` anterior).
    pub fn install(
        &self,
        bytes: &[u8],
        approved: &[Permission],
    ) -> Result<Installed, LifecycleError> {
        let pkg = superapp_package::verify(bytes, &self.trust)?;
        self.check_not_rollback(&pkg)?;
        let requested = normalize(&pkg.manifest.permissions);
        let approved = normalize(approved);
        if !approved.iter().all(|p| requested.contains(p)) {
            return Err(LifecycleError::ApprovalExceedsRequest);
        }
        let id = pkg.manifest.id.clone();
        let dir = self.app_dir(&id);
        std::fs::create_dir_all(&dir).map_err(io)?;
        write_atomic(&dir.join("package.sapp"), bytes)?;
        let state = State {
            approved: approved.clone(),
            enabled: true,
        };
        write_atomic(
            &dir.join("state.json"),
            &serde_json::to_vec(&state).map_err(io)?,
        )?;
        let installed = Installed {
            id: id.clone(),
            name: pkg.manifest.name.clone(),
            version: pkg.manifest.version.clone(),
            requested,
            approved,
            enabled: true,
            problem: None,
        };
        {
            let mut g = self.inner.lock().expect("registry");
            g.loaded.remove(&id);
            g.apps.insert(id.clone(), installed.clone());
        }
        // Compila já na instalação (o usuário está esperando a confirmação), não no primeiro
        // clique: componentes grandes (ex.: guest JS de ~11 MB) levam segundos para compilar.
        // Se falhar (ex.: permissão negada pelo usuário), a instalação continua válida e o erro
        // aparece ao abrir o app.
        if let Err(e) = self.loaded(&id) {
            tracing::info!(app = %id, error = %e, "pré-carga na instalação não concluída");
        }
        Ok(installed)
    }

    pub fn list(&self) -> Vec<Installed> {
        let g = self.inner.lock().expect("registry");
        let mut v: Vec<_> = g.apps.values().cloned().collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    /// Suspender (`false`) ou retomar (`true`). Não apaga nada.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<Installed, LifecycleError> {
        let mut g = self.inner.lock().expect("registry");
        let entry = g.apps.get_mut(id).ok_or(LifecycleError::NotInstalled)?;
        if let Some(p) = &entry.problem {
            return Err(LifecycleError::Broken(p.clone()));
        }
        entry.enabled = enabled;
        let state = State {
            approved: entry.approved.clone(),
            enabled,
        };
        let out = entry.clone();
        write_atomic(
            &self.app_dir(id).join("state.json"),
            &serde_json::to_vec(&state).map_err(io)?,
        )?;
        if !enabled {
            g.loaded.remove(id);
        }
        Ok(out)
    }

    /// Remove o app e, a menos que `keep_data`, os dados dele.
    pub fn uninstall(&self, id: &str, keep_data: bool) -> Result<(), LifecycleError> {
        if !superapp_package::valid_app_id(id) {
            return Err(LifecycleError::NotInstalled);
        }
        let mut g = self.inner.lock().expect("registry");
        if g.apps.remove(id).is_none() {
            return Err(LifecycleError::NotInstalled);
        }
        g.loaded.remove(id);
        std::fs::remove_dir_all(self.app_dir(id)).map_err(io)?;
        if !keep_data {
            let data = self.root.join("data").join(id);
            if data.exists() {
                std::fs::remove_dir_all(data).map_err(io)?;
            }
        }
        Ok(())
    }

    fn loaded(&self, id: &str) -> Result<(Arc<MiniApp>, Limits), LifecycleError> {
        let mut g = self.inner.lock().expect("registry");
        let entry = g.apps.get(id).ok_or(LifecycleError::NotInstalled)?.clone();
        if let Some(p) = &entry.problem {
            return Err(LifecycleError::Broken(p.clone()));
        }
        if !entry.enabled {
            return Err(LifecycleError::Disabled);
        }
        if let Some(l) = g.loaded.get(id) {
            return Ok((l.app.clone(), l.limits.clone()));
        }
        // Reverifica o pacote em disco contra a TrustStore atual antes de carregar.
        let pkg = self.verified_from_disk(id)?;
        let storage = DirStorage::open(self.root.join("data"), id).map_err(LifecycleError::Io)?;
        let app = Arc::new(self.runtime.load_verified(
            &pkg,
            &Grants::from_permissions(&entry.approved),
            Arc::new(storage),
        )?);
        let limits = Limits::from_manifest(&pkg.manifest);
        g.loaded.insert(
            id.to_owned(),
            Loaded {
                app: app.clone(),
                limits: limits.clone(),
            },
        );
        Ok((app, limits))
    }

    /// Entrega um evento ao mini-app e devolve a UI validada.
    pub async fn dispatch(&self, id: &str, event: &Event) -> Result<Node, LifecycleError> {
        event.validate().map_err(LifecycleError::InvalidUi)?;
        let (app, limits) = self.loaded(id)?;
        let input = serde_json::to_string(event).map_err(io)?;
        let out = app.run(&input, &limits).await?;
        Node::parse(&out.output).map_err(LifecycleError::InvalidUi)
    }
}
