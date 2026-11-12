//! Uma janela nativa por mini-app (o jeito "nativo" de ter vários mini-apps abertos: barra de
//! tarefas, Alt+Tab, Ctrl+W; no macOS as janelas se agrupam em abas nativas do sistema).
//!
//! Segurança:
//! - O rótulo da janela é `app-<id>` e é **sempre criado pelo host** (a UI só pede "abrir o app
//!   X", e o id é validado contra os apps instalados). A política dá `view:run` por prefixo
//!   de rótulo a essas janelas, e a UI do shell nelas não tem outra capability.
//! - `view.dispatch` **não aceita id**: o app é derivado do rótulo da janela de quem chama,
//!   então a janela de um mini-app só consegue falar com ele mesmo.
//! - Suspender ou remover um app fecha a janela dele.
use crate::apps::{map_err, AppsState};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use superapp_core::{ErrorCode, IpcError, Router};
use superapp_wasm_host::ui::Event;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Prefixo do rótulo das janelas de mini-app.
pub const PREFIX: &str = "app-";
/// Capability das janelas de mini-app: executar eventos do **próprio** app.
pub const CAP: &str = "view:run";
/// Teto de janelas de mini-app abertas ao mesmo tempo.
pub const MAX_WINDOWS: usize = 8;

pub fn label(id: &str) -> String {
    format!("{PREFIX}{id}")
}

/// Id do app a partir do rótulo da janela; `None` se não for uma janela de mini-app válida.
pub fn id_from_label(label: &str) -> Option<&str> {
    label
        .strip_prefix(PREFIX)
        .filter(|id| superapp_package::valid_app_id(id))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewArgs {
    event: Event,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenArgs {
    id: String,
}

/// Abre (ou traz para a frente) a janela do mini-app.
pub fn open(app: &AppHandle, st: &AppsState, id: &str) -> Result<(), IpcError> {
    if !superapp_package::valid_app_id(id) {
        return Err(IpcError::new(ErrorCode::InvalidArgs, "id inválido"));
    }
    let installed = st
        .registry()
        .list()
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "mini-app não instalado"))?;
    if let Some(p) = &installed.problem {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            format!("mini-app com problema: {p}"),
        ));
    }
    if !installed.enabled {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "mini-app suspenso",
        ));
    }
    let label = label(id);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }
    let open_now = app
        .webview_windows()
        .keys()
        .filter(|l| l.starts_with(PREFIX))
        .count();
    if open_now >= MAX_WINDOWS {
        return Err(IpcError::new(
            ErrorCode::InvalidArgs,
            format!("limite de {MAX_WINDOWS} janelas de mini-apps: feche alguma"),
        ));
    }
    let (h, name) = (app.clone(), installed.name);
    // Criar janela dentro de um comando/evento síncrono trava no Windows: faz em outra thread.
    std::thread::spawn(move || {
        let b = WebviewWindowBuilder::new(&h, &label, WebviewUrl::App("app.html".into()))
            .title(name)
            .inner_size(560.0, 640.0)
            .min_inner_size(320.0, 240.0);
        #[cfg(target_os = "macos")]
        let b = b.tabbing_identifier("superapp-apps");
        match b.build() {
            Ok(w) => {
                if !cfg!(target_os = "macos") {
                    let _ = w.hide_menu();
                }
            }
            Err(e) => tracing::error!(error = %e, "falha ao abrir a janela do mini-app"),
        }
    });
    Ok(())
}

/// Fecha a janela do mini-app, se aberta (ao suspender ou remover o app).
pub fn close(app: &AppHandle, id: &str) {
    if let Some(w) = app.get_webview_window(&label(id)) {
        let _ = w.destroy();
    }
}

pub fn register(r: &mut Router, st: Arc<AppsState>, app: AppHandle) {
    let s = st.clone();
    r.register("apps.open", crate::apps::CAP, move |_c, a: OpenArgs| {
        open(&app, &s, &a.id)?;
        Ok(Value::Null)
    });

    // O app vem do rótulo da janela de quem chama, nunca dos argumentos.
    r.register("view.dispatch", CAP, move |c, a: ViewArgs| {
        let id = id_from_label(&c.window)
            .ok_or_else(|| IpcError::new(ErrorCode::PermissionDenied, "janela sem mini-app"))?;
        tauri::async_runtime::block_on(st.registry().dispatch(id, &a.event)).map_err(map_err)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_roundtrip_and_reject_anything_that_is_not_an_app_window() {
        assert_eq!(label("clicker"), "app-clicker");
        assert_eq!(id_from_label("app-clicker"), Some("clicker"));
        assert_eq!(id_from_label("app-a-b-1"), Some("a-b-1"));
        for bad in [
            "main",
            "untrusted",
            "mfe-a",
            "app-",
            "app-../x",
            "app-A",
            "app-a b",
            "xapp-a",
            "",
        ] {
            assert_eq!(id_from_label(bad), None, "{bad:?}");
        }
        assert_eq!(id_from_label(&format!("app-{}", "a".repeat(33))), None);
    }
}
