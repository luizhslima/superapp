//! Chaves de autores confiáveis (E-casca): ver, adicionar e remover pela UI, sem editar
//! `trusted-keys.txt` à mão.
//!
//! Regras:
//! - Só a janela principal tem a capability (`trust:manage`); o código de mini-apps não chega
//!   aqui (não tem IPC). A UI confirma em dois cliques antes de adicionar.
//! - A mudança vale **depois de reiniciar** o Superapp: o conjunto de chaves é carregado na
//!   abertura e usado por pacotes e catálogo, e trocá-lo com apps rodando deixaria o estado
//!   ambíguo. A tela mostra o que ainda está pendente.
//! - Remover uma chave faz os apps dela pararem de rodar no próximo início (o pacote é
//!   reverificado a cada carga). É a revogação do uso pessoal: sem lista central.
//! - O arquivo é editado só por linhas: comentários e a ordem do usuário são preservados.
use crate::apps::AppsState;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use superapp_core::{ErrorCode, IpcError, Router};
use superapp_package::{parse_hex32, to_hex};

/// Capability dos comandos de chaves. Só a janela principal a recebe.
pub const CAP: &str = "trust:manage";
const FILE: &str = "trusted-keys.txt";
const MAX_KEYS: usize = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyArgs {
    key: String,
}

fn bad(msg: &str) -> IpcError {
    IpcError::new(ErrorCode::InvalidArgs, format!("chaves: {msg}"))
}

fn internal(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Internal, format!("chaves: {e}"))
}

/// Chave de uma linha (sem comentário), em hex minúsculo; `None` para linha vazia/inválida.
fn line_key(line: &str) -> Option<String> {
    let l = line.split('#').next().unwrap_or("").trim();
    parse_hex32(l).map(|k| to_hex(&k))
}

/// Chaves do arquivo, em ordem.
pub fn file_keys(text: &str) -> Vec<String> {
    text.lines().filter_map(line_key).collect()
}

/// Devolve o texto com a chave acrescentada (ou erro se já existe ou passa do limite).
pub fn with_key(text: &str, key: &[u8; 32]) -> Result<String, String> {
    let hex = to_hex(key);
    let existing = file_keys(text);
    if existing.contains(&hex) {
        return Err("esta chave já está na lista".into());
    }
    if existing.len() >= MAX_KEYS {
        return Err(format!("limite de {MAX_KEYS} chaves"));
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("{hex} # adicionada pelo Superapp\n"));
    Ok(out)
}

/// Devolve o texto sem as linhas dessa chave (comentários e outras chaves ficam).
pub fn without_key(text: &str, key: &[u8; 32]) -> String {
    let hex = to_hex(key);
    let mut out = String::new();
    for line in text.lines() {
        if line_key(line).as_deref() != Some(hex.as_str()) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn read(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(FILE)).unwrap_or_default()
}

fn write(dir: &Path, text: &str) -> Result<(), IpcError> {
    let tmp = dir.join("trusted-keys.tmp");
    std::fs::write(&tmp, text).map_err(internal)?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(internal)
}

fn parse_key(a: &KeyArgs) -> Result<[u8; 32], IpcError> {
    parse_hex32(a.key.trim())
        .ok_or_else(|| bad("chave inválida (esperado 64 caracteres hexadecimais)"))
}

pub fn register(r: &mut Router, apps: Arc<AppsState>) {
    let s = apps.clone();
    r.register("trust.list", CAP, move |_c, _a: Value| {
        let in_file = file_keys(&read(s.data_dir()));
        let active: Vec<String> = s.trust().keys().iter().map(|k| to_hex(k)).collect();
        let mut keys: Vec<Value> = in_file
            .iter()
            .map(|k| json!({ "key": k, "in_file": true, "active": active.contains(k) }))
            .collect();
        // Removidas do arquivo mas ainda valendo até reiniciar.
        for k in active.iter().filter(|k| !in_file.contains(k)) {
            keys.push(json!({ "key": k, "in_file": false, "active": true }));
        }
        let pending = keys.iter().any(|k| k["in_file"] != k["active"]);
        Ok(json!({ "keys": keys, "pending_restart": pending }))
    });

    let s = apps.clone();
    r.register("trust.add", CAP, move |_c, a: KeyArgs| {
        let key = parse_key(&a)?;
        let text = with_key(&read(s.data_dir()), &key).map_err(|e| bad(&e))?;
        write(s.data_dir(), &text)?;
        Ok(Value::Null)
    });

    r.register("trust.remove", CAP, move |_c, a: KeyArgs| {
        let key = parse_key(&a)?;
        let before = read(apps.data_dir());
        if !file_keys(&before).contains(&to_hex(&key)) {
            return Err(IpcError::new(
                ErrorCode::NotFound,
                "chave não está na lista",
            ));
        }
        write(apps.data_dir(), &without_key(&before, &key))?;
        Ok(Value::Null)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: [u8; 32] = [0xAA; 32];
    const B: [u8; 32] = [0xBB; 32];

    #[test]
    fn adding_appends_a_line_and_keeps_the_rest() {
        let t = with_key("# meus autores\n", &A).unwrap();
        assert!(t.starts_with("# meus autores\n"));
        assert_eq!(file_keys(&t), vec![to_hex(&A)]);
        // Arquivo sem quebra de linha no fim também funciona.
        let t2 = with_key(&format!("{} # eu", to_hex(&A)), &B).unwrap();
        assert_eq!(file_keys(&t2), vec![to_hex(&A), to_hex(&B)]);
        assert!(t2.contains("# eu\n"));
    }

    #[test]
    fn duplicates_and_the_limit_are_refused() {
        let t = with_key("", &A).unwrap();
        assert!(with_key(&t, &A).is_err());
        let mut full = String::new();
        for i in 0..MAX_KEYS {
            full = with_key(&full, &[i as u8 + 1; 32]).unwrap();
        }
        assert!(with_key(&full, &[0xEE; 32]).is_err());
    }

    #[test]
    fn removing_drops_only_that_key_and_keeps_comments() {
        let t = format!("# topo\n{} # a\n\n{} # b\n", to_hex(&A), to_hex(&B));
        let r = without_key(&t, &A);
        assert_eq!(file_keys(&r), vec![to_hex(&B)]);
        assert!(r.contains("# topo"));
        assert!(!r.contains("# a\n"));
    }

    #[test]
    fn result_is_always_a_valid_trust_file() {
        let t = with_key("", &A).unwrap();
        let t = with_key(&t, &B).unwrap();
        let store = superapp_package::TrustStore::from_hex_lines(&t).unwrap();
        assert!(store.is_trusted(&A) && store.is_trusted(&B));
        let t = without_key(&t, &A);
        let store = superapp_package::TrustStore::from_hex_lines(&t).unwrap();
        assert!(!store.is_trusted(&A) && store.is_trusted(&B));
    }

    #[test]
    fn uppercase_hex_is_normalized() {
        let upper = to_hex(&A).to_uppercase();
        assert_eq!(file_keys(&upper), vec![to_hex(&A)]);
        assert!(with_key(&upper, &A).is_err());
    }
}
