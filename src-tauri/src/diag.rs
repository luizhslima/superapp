//! Relatório de diagnóstico local (E5.3).
//!
//! Nada sai sozinho: o relatório só é montado quando o usuário pede, aparece na tela e ele
//! decide se copia e envia. Contém apenas dados técnicos (versão, sistema, contagem de
//! decisões do roteador, últimas negações e mini-apps instalados). Não contém caminhos,
//! nome de usuário, dados dos mini-apps nem conteúdo de mensagens.
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use superapp_core::{AuditEvent, AuditSink, Decision, Router};

/// Capability do comando. Só a janela principal a recebe.
pub const CAP: &str = "diag:read";
/// Quantas negações recentes ficam na memória (o resto é descartado: tamanho fixo).
const RECENT: usize = 50;

#[derive(Default)]
struct State {
    counts: BTreeMap<String, u64>,
    recent_denials: VecDeque<(String, String, String)>,
}

/// Sink de auditoria com memória de tamanho fixo: contadores por decisão e as últimas negações.
#[derive(Default)]
pub struct DiagAudit {
    state: Mutex<State>,
}

impl AuditSink for DiagAudit {
    fn record(&self, e: AuditEvent) {
        let mut s = self.state.lock().expect("diag");
        *s.counts.entry(format!("{:?}", e.decision)).or_insert(0) += 1;
        if e.decision != Decision::Allowed {
            if s.recent_denials.len() == RECENT {
                s.recent_denials.pop_front();
            }
            // Nomes de negações já chegam sanitizados pelo Router.
            s.recent_denials
                .push_back((format!("{:?}", e.decision), e.window, e.command));
        }
    }
}

impl DiagAudit {
    fn snapshot(&self) -> (BTreeMap<String, u64>, Vec<Value>) {
        let s = self.state.lock().expect("diag");
        let recent = s
            .recent_denials
            .iter()
            .map(|(d, w, c)| json!({ "decision": d, "window": w, "command": c }))
            .collect();
        (s.counts.clone(), recent)
    }
}

pub fn register(
    r: &mut Router,
    diag: Arc<DiagAudit>,
    start: std::time::Instant,
    app: tauri::AppHandle,
) {
    use tauri::Manager;
    r.register("diag.report", CAP, move |_c, _a: Value| {
        let (counts, recent) = diag.snapshot();
        let apps = app
            .try_state::<Arc<crate::apps::AppsState>>()
            .map(|s| s.summary())
            .unwrap_or_default();
        Ok(json!({
            "report_version": 1,
            "app_version": superapp_core::VERSION,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "uptime_seconds": start.elapsed().as_secs(),
            "decisions": counts,
            "recent_denials": recent,
            "mini_apps": apps,
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(decision: Decision, command: &str) -> AuditEvent {
        AuditEvent {
            window: "w".into(),
            command: command.into(),
            decision,
        }
    }

    #[test]
    fn counts_decisions_and_keeps_only_denials_in_the_ring() {
        let d = DiagAudit::default();
        d.record(ev(Decision::Allowed, "ok"));
        d.record(ev(Decision::DeniedNoCapability, "x"));
        let (counts, recent) = d.snapshot();
        assert_eq!(counts["Allowed"], 1);
        assert_eq!(counts["DeniedNoCapability"], 1);
        assert_eq!(recent.len(), 1);
    }

    #[test]
    fn ring_has_fixed_size() {
        let d = DiagAudit::default();
        for i in 0..(RECENT * 3) {
            d.record(ev(Decision::DeniedUnknownCommand, &format!("c{i}")));
        }
        let (counts, recent) = d.snapshot();
        assert_eq!(recent.len(), RECENT);
        assert_eq!(counts["DeniedUnknownCommand"], (RECENT * 3) as u64);
        // Fica com as mais recentes.
        assert_eq!(
            recent.last().unwrap()["command"],
            format!("c{}", RECENT * 3 - 1)
        );
    }
}
