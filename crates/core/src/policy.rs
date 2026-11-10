//! Política de capabilities: zero privilégio por padrão (ADR-006).
use std::collections::{HashMap, HashSet};

/// Quem está chamando: a janela (label do Tauri) de onde veio o comando.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Caller {
    pub window: String,
}

impl Caller {
    pub fn new(window: impl Into<String>) -> Self {
        Self {
            window: window.into(),
        }
    }
}

/// Mapa janela → capabilities concedidas. Janela desconhecida não tem nada.
#[derive(Debug, Default, Clone)]
pub struct Policy {
    grants: HashMap<String, HashSet<String>>,
}

impl Policy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant(mut self, window: &str, capability: &str) -> Self {
        self.grants
            .entry(window.to_owned())
            .or_default()
            .insert(capability.to_owned());
        self
    }

    pub fn allows(&self, caller: &Caller, capability: &str) -> bool {
        self.grants
            .get(&caller.window)
            .is_some_and(|set| set.contains(capability))
    }
}
