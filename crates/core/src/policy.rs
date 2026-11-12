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
///
/// Além de rótulos exatos, aceita concessão por **prefixo** de rótulo, para janelas criadas
/// dinamicamente pelo próprio host (ex.: uma por mini-app, `app-<id>`). Só o código do host
/// cria janelas, e o rótulo vem sempre dele, nunca da UI.
#[derive(Debug, Default, Clone)]
pub struct Policy {
    grants: HashMap<String, HashSet<String>>,
    prefix_grants: Vec<(String, String)>,
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

    /// Concede `capability` a toda janela cujo rótulo começa com `prefix` (não vazio).
    pub fn grant_prefix(mut self, prefix: &str, capability: &str) -> Self {
        assert!(
            !prefix.is_empty(),
            "prefixo vazio concederia a todas as janelas"
        );
        self.prefix_grants
            .push((prefix.to_owned(), capability.to_owned()));
        self
    }

    pub fn allows(&self, caller: &Caller, capability: &str) -> bool {
        self.grants
            .get(&caller.window)
            .is_some_and(|set| set.contains(capability))
            || self
                .prefix_grants
                .iter()
                .any(|(p, c)| c == capability && caller.window.starts_with(p.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_grants_apply_only_to_matching_windows_and_capabilities() {
        let p = Policy::new()
            .grant("main", "app:read")
            .grant_prefix("app-", "view:run");
        let c = |w: &str| Caller::new(w);
        assert!(p.allows(&c("app-clicker"), "view:run"));
        assert!(p.allows(&c("app-x"), "view:run"));
        // Outra capability, outro rótulo, ou rótulo que só *contém* o prefixo: nada.
        assert!(!p.allows(&c("app-clicker"), "app:read"));
        assert!(!p.allows(&c("main"), "view:run"));
        assert!(!p.allows(&c("xapp-clicker"), "view:run"));
        assert!(!p.allows(&c("untrusted"), "view:run"));
        // Concessão exata continua funcionando.
        assert!(p.allows(&c("main"), "app:read"));
    }

    #[test]
    #[should_panic]
    fn empty_prefix_is_refused() {
        let _ = Policy::new().grant_prefix("", "view:run");
    }
}
