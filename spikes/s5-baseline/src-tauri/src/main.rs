// Spike S5: app mínimo para medir cold start, RAM e tamanho.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[tauri::command]
fn ready() {
    if let Ok(path) = std::env::var("S5_READY") {
        let _ = std::fs::write(path, "ready");
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ready])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o app");
}
