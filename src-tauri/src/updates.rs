//! Atualização do próprio host (E5.2), exposta só pelo Router.
//!
//! Segurança:
//! - O endereço do catálogo (`latest.json`) e a chave pública ficam fixos em `tauri.conf.json`;
//!   a UI não informa URL nenhuma, então não consegue apontar o host para outro servidor.
//! - O plugin exige HTTPS e confere a assinatura minisign do pacote contra a chave embutida;
//!   sem assinatura válida nada é instalado.
//! - Os comandos nativos do plugin não recebem permissão em nenhuma capability: o único
//!   caminho é `app.update.*`, que passa por política e auditoria do Router.
use serde::Serialize;
use serde_json::Value;
use superapp_core::{ErrorCode, IpcError, Router};
use tauri_plugin_updater::UpdaterExt;

/// Capability dos comandos de atualização. Só a janela principal a recebe.
pub const CAP: &str = "app:update";

#[derive(Serialize)]
struct CheckResult {
    current: String,
    available: Option<String>,
    notes: Option<String>,
}

fn fail(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Internal, format!("atualização: {e}"))
}

pub fn register(r: &mut Router, app: tauri::AppHandle) {
    let h = app.clone();
    r.register("app.update.check", CAP, move |_c, _a: Value| {
        tauri::async_runtime::block_on(async {
            let updater = h.updater().map_err(fail)?;
            let found = updater.check().await.map_err(fail)?;
            Ok(CheckResult {
                current: h.package_info().version.to_string(),
                available: found.as_ref().map(|u| u.version.clone()),
                notes: found.and_then(|u| u.body),
            })
        })
    });

    // Reinicia o app (usado após mudar as chaves confiáveis). Não recebe argumento algum.
    let h = app.clone();
    r.register("app.restart", CAP, move |_c, _a: Value| {
        h.restart();
        #[allow(unreachable_code)]
        Ok::<Value, IpcError>(Value::Null)
    });

    // Encerra o app (menu Arquivo > Sair). Não recebe argumento algum.
    let h = app.clone();
    r.register("app.quit", CAP, move |_c, _a: Value| {
        h.exit(0);
        Ok::<Value, IpcError>(Value::Null)
    });

    // Baixa, confere a assinatura, instala e reinicia. Só age sobre o que o `check` achou
    // agora: não aceita versão nem URL da UI.
    r.register("app.update.install", CAP, move |_c, _a: Value| {
        tauri::async_runtime::block_on(async {
            let updater = app.updater().map_err(fail)?;
            let Some(update) = updater.check().await.map_err(fail)? else {
                return Err::<Value, IpcError>(IpcError::new(
                    ErrorCode::NotFound,
                    "nenhuma atualização",
                ));
            };
            update
                .download_and_install(|_, _| {}, || {})
                .await
                .map_err(fail)?;
            app.restart();
        })
    });
}
