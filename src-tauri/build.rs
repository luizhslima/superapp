fn main() {
    // Só o comando `ipc` existe para a UI; tudo passa pelo Router do core.
    let attrs = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(&["ipc", "ipc_bytes"]));
    tauri_build::try_build(attrs).expect("falha no tauri-build");
}
