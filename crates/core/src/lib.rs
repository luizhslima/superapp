//! Núcleo do host: roteador IPC, capabilities e ciclo de vida (preenchido na Fase 1).

/// Versão do núcleo, usada em handshakes e diagnósticos.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_set() {
        assert!(!VERSION.is_empty());
    }
}
