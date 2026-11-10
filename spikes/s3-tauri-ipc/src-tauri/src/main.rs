// Spike S3: Isolation Pattern + Binary IPC + multi-webview.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
use tauri::ipc::Response;

const SIZE: usize = 10 * 1024 * 1024;

fn payload() -> Vec<u8> {
    (0..SIZE).map(|i| (i % 251) as u8).collect()
}

/// Caminho JSON: Vec<u8> vira array de números no JSON.
#[tauri::command]
fn blob_json() -> Vec<u8> {
    payload()
}

/// Caminho binário: resposta raw, sem serialização JSON.
#[tauri::command]
fn blob_binary() -> Response {
    Response::new(payload())
}

/// Anexa o resultado ao arquivo indicado por S3_OUT e, se `done`, encerra o app.
#[tauri::command]
fn report(app: tauri::AppHandle, line: String, done: bool) {
    if let Ok(path) = std::env::var("S3_OUT") {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
    if done {
        app.exit(0);
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![blob_json, blob_binary, report])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o app");
}
