//! Barra de menu nativa da janela principal.
//!
//! O menu **não executa nada sozinho**: cada item só emite um evento com um id de uma lista
//! fixa ([`ACTIONS`]) para a janela principal, e é a tela que reage chamando os mesmos
//! comandos de sempre (`apps.pick`, `app.update.check`...), que passam pela política e pela
//! auditoria do Router. Assim o menu não abre um segundo caminho para ações privilegiadas.
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter};

/// Nome do evento recebido pela tela (`window.__TAURI__.event.listen`).
pub const EVENT: &str = "menu://action";

/// Todos os ids que o menu pode emitir. A tela trata exatamente estes (há teste de consistência).
pub const ACTIONS: &[&str] = &[
    "pick",
    "close-tab",
    "tab-apps",
    "tab-store",
    "tab-settings",
    "next-tab",
    "prev-tab",
    "update-check",
    "diag",
];

fn item(
    app: &AppHandle,
    id: &str,
    label: &str,
    accel: Option<&str>,
) -> tauri::Result<tauri::menu::MenuItem<tauri::Wry>> {
    debug_assert!(ACTIONS.contains(&id));
    let b = MenuItemBuilder::with_id(id, label);
    match accel {
        Some(a) => b.accelerator(a).build(app),
        None => b.build(app),
    }
}

/// Monta e instala o menu. Falha aqui não derruba o app (a tela funciona sem menu).
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let file = SubmenuBuilder::new(app, "Arquivo")
        .item(&item(
            app,
            "pick",
            "Instalar mini-app de um arquivo...",
            Some("CmdOrCtrl+O"),
        )?)
        .item(&item(app, "close-tab", "Fechar aba", Some("CmdOrCtrl+W"))?)
        .separator()
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
        .item(&item(app, "tab-apps", "Mini-apps", Some("CmdOrCtrl+1"))?)
        .item(&item(app, "tab-store", "Loja", Some("CmdOrCtrl+2"))?)
        .item(&item(
            app,
            "tab-settings",
            "Configurações",
            Some("CmdOrCtrl+3"),
        )?)
        .separator()
        .item(&item(app, "next-tab", "Próxima aba", Some("Ctrl+Tab"))?)
        .item(&item(
            app,
            "prev-tab",
            "Aba anterior",
            Some("Ctrl+Shift+Tab"),
        )?)
        .build()?;
    let help = SubmenuBuilder::new(app, "Ajuda")
        .item(&item(app, "update-check", "Verificar atualizações", None)?)
        .item(&item(app, "diag", "Relatório de diagnóstico", None)?)
        .build()?;
    let menu = MenuBuilder::new(app)
        .items(&[&file, &edit, &view, &help])
        .build()?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        let id = event.id().as_ref();
        // Só ids conhecidos saem daqui; o resto (itens predefinidos como copiar) é do sistema.
        if ACTIONS.contains(&id) {
            let _ = app.emit_to("main", EVENT, id);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cada ação do menu precisa ser tratada pela tela, e o evento escutado tem o mesmo nome.
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

    /// O hambúrguer da tela usa os mesmos ids do menu nativo: nenhum item pode ficar órfão.
    #[test]
    fn hamburger_items_use_known_actions() {
        let html = include_str!("../../ui/shell/index.html");
        let used: Vec<&str> = html
            .split("data-action=\"")
            .skip(1)
            .filter_map(|r| r.split('"').next())
            .collect();
        assert!(!used.is_empty(), "o hambúrguer não tem itens");
        for id in used {
            assert!(
                ACTIONS.contains(&id),
                "ação desconhecida no hambúrguer: {id}"
            );
        }
    }
}
