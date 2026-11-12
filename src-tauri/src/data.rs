//! Backup e restauração dos dados de um mini-app, e escolha de um `.sapp` local.
//!
//! O caminho do arquivo **nunca** vem da UI: o Rust abre o diálogo nativo e lê/grava o que o
//! usuário escolher ali. A UI só diz "exportar o app X" / "importar para o app X" / "escolher
//! um pacote". A validação do backup (id do app, limites, quota) está em
//! `superapp_wasm_host::backup` e acontece antes de qualquer escrita.
use crate::apps::AppsState;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use superapp_core::{ErrorCode, IpcError, Router};
use superapp_wasm_host::backup::MAX_BACKUP_BYTES;
use superapp_wasm_host::lifecycle::LifecycleError;
use tauri_plugin_dialog::DialogExt;

/// Capability de backup e de escolha de arquivo. Só a janela principal a recebe.
pub const CAP: &str = "data:backup";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArgs {
    id: String,
}

fn lc(e: LifecycleError) -> IpcError {
    match e {
        LifecycleError::NotInstalled => {
            IpcError::new(ErrorCode::NotFound, "mini-app não instalado")
        }
        other => IpcError::new(ErrorCode::InvalidArgs, format!("backup: {other}")),
    }
}

fn io(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Internal, format!("backup: {e}"))
}

pub fn register(r: &mut Router, apps: Arc<AppsState>, app: tauri::AppHandle) {
    // Exporta: gera o JSON, pede onde salvar. `saved: false` = o usuário cancelou.
    let (a, h) = (apps.clone(), app.clone());
    r.register("data.export", CAP, move |_c, args: IdArgs| {
        let bytes = a.registry().export_data(&args.id).map_err(lc)?;
        let Some(path) = h
            .dialog()
            .file()
            .add_filter("Backup do Superapp", &["json"])
            .set_file_name(format!("{}-backup.json", args.id))
            .blocking_save_file()
        else {
            return Ok(json!({ "saved": false }));
        };
        let path = path.into_path().map_err(io)?;
        std::fs::write(path, &bytes).map_err(io)?;
        Ok(json!({ "saved": true }))
    });

    // Importa: pede o arquivo, valida tudo e troca os dados do app. `restored: null` = cancelou.
    let (a, h) = (apps.clone(), app.clone());
    r.register("data.import", CAP, move |_c, args: IdArgs| {
        let Some(path) = h
            .dialog()
            .file()
            .add_filter("Backup do Superapp", &["json"])
            .blocking_pick_file()
        else {
            return Ok(json!({ "restored": Value::Null }));
        };
        let path = path.into_path().map_err(io)?;
        let meta = std::fs::metadata(&path).map_err(io)?;
        if !meta.is_file() || meta.len() > MAX_BACKUP_BYTES as u64 {
            return Err(IpcError::new(
                ErrorCode::InvalidArgs,
                "backup: arquivo inválido ou grande demais",
            ));
        }
        let bytes = std::fs::read(path).map_err(io)?;
        let n = a.registry().import_data(&args.id, &bytes).map_err(lc)?;
        Ok(json!({ "restored": n }))
    });

    // Escolher um `.sapp` local (alternativa a arrastar): vai para o mesmo consentimento.
    r.register("apps.pick", CAP, move |_c, _a: Value| {
        let Some(path) = app
            .dialog()
            .file()
            .add_filter("Mini-app do Superapp", &["sapp"])
            .blocking_pick_file()
        else {
            return Ok(json!({ "picked": false }));
        };
        let path = path.into_path().map_err(io)?;
        apps.stage_path(&path)
            .map_err(|e| IpcError::new(ErrorCode::InvalidArgs, format!("pacote recusado: {e}")))?;
        Ok(json!({ "picked": true }))
    });
}
