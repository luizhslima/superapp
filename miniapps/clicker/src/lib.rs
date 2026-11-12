//! Mini-app de exemplo com UI declarativa, escrito com o SDK (ADR-009).
//! Só usa `storage`, então só essa permissão é exigida.
use superapp_sdk::{export_app, storage, ui, App, Event, Node};

struct Clicker;

impl App for Clicker {
    fn handle(event: Event) -> Result<Node, String> {
        let mut n = storage::get_u64("clicks").unwrap_or(0);
        if matches!(&event, Event::Click { id, .. } if id == "inc") {
            n += 1;
            storage::set_u64("clicks", n)?;
        }
        Ok(ui::stack(vec![
            ui::text(format!("Cliques: {n}")),
            ui::button("inc", "Incrementar"),
        ]))
    }
}

export_app!(Clicker);
