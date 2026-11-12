//! Menu nativo do app. No Windows e no Linux a barra tem **um item só, "☰"**, com Arquivo,
//! Editar, Exibir e Ajuda dentro (compacto); no macOS vão os quatro menus padrão na barra do
//! sistema. Atalhos são aceleradores nativos.
//!
//! O menu **não executa nada privilegiado sozinho**:
//! - "Abrir mini-app" (`open:<id>`) e "Fechar janela" são tratados aqui no Rust, com o id
//!   validado contra os apps instalados;
//! - os demais itens só emitem um id de uma lista fixa ([`ACTIONS`]) para a janela principal,
//!   que reage chamando os mesmos comandos de sempre (`apps.pick`, `app.update.check`...),
//!   que passam pela política e pela auditoria do Router.
use crate::apps::AppsState;
use crate::appwin;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use superapp_core::Router;
use superapp_wasm_host::lifecycle::Installed;
use tauri::menu::{AboutMetadata, ContextMenu, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager};

/// Nome do evento recebido pela janela principal (`window.__TAURI__.event.listen`).
pub const EVENT: &str = "menu://action";

/// Ids que o menu emite para a janela principal. A tela trata exatamente estes (há teste).
pub const ACTIONS: &[&str] = &[
    "pick",
    "sec-apps",
    "sec-store",
    "sec-settings",
    "update-check",
    "diag",
];

/// Capability do botão ☰ da barra de topo (só a janela principal): abre o menu nativo como pop-up.
pub const CAP: &str = "menu:use";

/// Menu em formato pop-up (os 4 menus direto, sem o item ☰), reconstruído junto com o da barra.
#[derive(Default)]
pub struct MenuState {
    popup: Mutex<Option<Menu<tauri::Wry>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// Barra de menu da janela (no Windows/Linux: um item ☰ só).
    Bar,
    /// Pop-up do botão ☰: os quatro menus direto.
    Popup,
}

const OPEN_PREFIX: &str = "open:";
const CLOSE_WINDOW: &str = "close-window";

fn item(
    app: &AppHandle,
    id: &str,
    label: &str,
    accel: Option<&str>,
) -> tauri::Result<tauri::menu::MenuItem<tauri::Wry>> {
    let b = MenuItemBuilder::with_id(id, label);
    match accel {
        Some(a) => b.accelerator(a).build(app),
        None => b.build(app),
    }
}

/// Monta o menu inteiro a partir dos mini-apps instalados.
fn build(app: &AppHandle, apps: &[Installed], layout: Layout) -> tauri::Result<Menu<tauri::Wry>> {
    // Arquivo > Abrir mini-app: um item por app ativo.
    let mut open = SubmenuBuilder::new(app, "Abrir mini-app");
    let usable: Vec<&Installed> = apps
        .iter()
        .filter(|a| a.enabled && a.problem.is_none())
        .collect();
    for a in &usable {
        open = open.item(&item(
            app,
            &format!("{OPEN_PREFIX}{}", a.id),
            &a.name,
            None,
        )?);
    }
    if usable.is_empty() {
        open = open.item(
            &MenuItemBuilder::with_id("no-apps", "Nenhum mini-app instalado")
                .enabled(false)
                .build(app)?,
        );
    }
    let file = SubmenuBuilder::new(app, "Arquivo")
        .item(&item(
            app,
            "pick",
            "Instalar mini-app de um arquivo...",
            Some("CmdOrCtrl+O"),
        )?)
        .item(&open.build()?)
        .separator()
        .item(&item(
            app,
            CLOSE_WINDOW,
            "Fechar janela",
            Some("CmdOrCtrl+W"),
        )?)
        .quit()
        .build()?;
    let edit = SubmenuBuilder::new(app, "Editar")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let view = SubmenuBuilder::new(app, "Exibir")
        .item(&item(app, "sec-apps", "Mini-apps", Some("CmdOrCtrl+1"))?)
        .item(&item(app, "sec-store", "Loja", Some("CmdOrCtrl+2"))?)
        .item(&item(
            app,
            "sec-settings",
            "Configurações",
            Some("CmdOrCtrl+3"),
        )?)
        .build()?;
    let about = AboutMetadata {
        name: Some("Superapp".into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        ..Default::default()
    };
    let help = SubmenuBuilder::new(app, "Ajuda")
        .item(&item(app, "update-check", "Verificar atualizações", None)?)
        .item(&item(app, "diag", "Relatório de diagnóstico", None)?)
        .separator()
        .about(Some(about))
        .build()?;
    if cfg!(target_os = "macos") || layout == Layout::Popup {
        MenuBuilder::new(app)
            .items(&[&file, &edit, &view, &help])
            .build()
    } else {
        // Um item só na barra: o "hambúrguer" nativo.
        let burger = SubmenuBuilder::new(app, "\u{2003}\u{2630}\u{2003}")
            .item(&file)
            .item(&edit)
            .item(&view)
            .item(&help)
            .build()?;
        MenuBuilder::new(app).item(&burger).build()
    }
}

/// Reconstrói o menu (a lista "Abrir mini-app" muda quando se instala, suspende ou remove).
pub fn rebuild(app: &AppHandle, st: &Arc<AppsState>) {
    let (h, st) = (app.clone(), st.clone());
    let _ = app.run_on_main_thread(move || {
        let apps = st.registry().list();
        match build(&h, &apps, Layout::Bar) {
            Ok(m) => {
                if let Err(e) = h.set_menu(m) {
                    tracing::error!(error = %e, "menu nativo não aplicado");
                }
            }
            Err(e) => tracing::error!(error = %e, "menu nativo não montado"),
        }
        // A barra fina some no Windows/Linux (o botão ☰ da página a substitui); os aceleradores
        // continuam no menu da janela.
        if !cfg!(target_os = "macos") {
            for w in h.webview_windows().values() {
                let _ = w.hide_menu();
            }
        }
        match build(&h, &apps, Layout::Popup) {
            Ok(m) => {
                if let Some(state) = h.try_state::<MenuState>() {
                    *state.popup.lock().expect("menu") = Some(m);
                }
            }
            Err(e) => tracing::error!(error = %e, "menu pop-up não montado"),
        }
    });
}

/// `menu.popup`: abre o menu nativo na posição do cursor (clique no botão ☰ da página).
pub fn register(r: &mut Router, app: AppHandle) {
    r.register("menu.popup", CAP, move |_c, _a: Value| {
        let h = app.clone();
        app.run_on_main_thread(move || {
            let (Some(w), Some(state)) = (h.get_webview_window("main"), h.try_state::<MenuState>())
            else {
                return;
            };
            let menu = state.popup.lock().expect("menu").clone();
            if let Some(m) = menu {
                let _ = m.popup(w.as_ref().window().clone());
            }
        })
        .map_err(|e| {
            superapp_core::IpcError::new(superapp_core::ErrorCode::Internal, e.to_string())
        })?;
        Ok(Value::Null)
    });
}

/// Liga o tratamento dos itens (uma vez só).
pub fn init(app: &AppHandle) {
    app.on_menu_event(|app, event| {
        let id = event.id().as_ref();
        if let Some(app_id) = id.strip_prefix(OPEN_PREFIX) {
            if let Some(st) = app.try_state::<Arc<AppsState>>() {
                if let Err(e) = appwin::open(app, &st, app_id) {
                    tracing::warn!(error = %e.message, "não abriu o mini-app pelo menu");
                }
            }
        } else if id == CLOSE_WINDOW {
            if let Some(w) = app
                .webview_windows()
                .values()
                .find(|w| w.is_focused().unwrap_or(false))
            {
                let _ = w.close();
            }
        } else if ACTIONS.contains(&id) {
            // As seções e ações vivem na janela principal: traz ela para a frente.
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.show();
                let _ = main.unminimize();
                let _ = main.set_focus();
            }
            let _ = app.emit_to("main", EVENT, id);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cada ação emitida pelo menu precisa ser tratada pela janela principal, e o evento
    /// escutado tem o mesmo nome.
    #[test]
    fn every_menu_action_is_handled_by_the_shell() {
        let js = include_str!("../../ui/shell/apps.js");
        assert!(js.contains(EVENT), "a tela não escuta {EVENT}");
        for a in ACTIONS {
            assert!(
                js.contains(&format!("case \"{a}\":")),
                "a tela não trata a ação {a}"
            );
        }
    }

    #[test]
    fn open_ids_always_carry_a_valid_app_id_check() {
        // O id depois de `open:` só vira janela se for um id de app válido e instalado
        // (`appwin::open` valida); ids hostis nunca chegam a criar rótulo.
        assert_eq!(appwin::id_from_label("app-../x"), None);
        assert!("open:clicker".starts_with(OPEN_PREFIX));
    }
}
