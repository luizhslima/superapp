//! Backup dos dados de um mini-app (E-casca): exportar e restaurar o armazenamento dele.
//!
//! Formato (JSON, legível): `{ format, app_id, app_version, created_at, entries: [{key, value_hex}] }`.
//! O arquivo é **não confiável** na volta: a restauração valida tudo (id do app, tamanho de
//! chave e valor, quota total, chaves repetidas) antes de tocar no disco e troca o diretório
//! de dados de uma vez (prepara ao lado, depois renomeia), então um backup ruim ou uma falha
//! no meio não deixam o app com dados pela metade.
use crate::storage::{MAX_KEY_BYTES, MAX_TOTAL_BYTES, MAX_VALUE_BYTES};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

pub const FORMAT: &str = "superapp-backup-1";
/// Teto do arquivo de backup (valores em hex dobram o tamanho: 4 MiB de dados ≈ 8 MiB).
pub const MAX_BACKUP_BYTES: usize = 12 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 4096;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    format: String,
    app_id: String,
    app_version: String,
    created_at: u64,
    entries: Vec<Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    key: String,
    value_hex: String,
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Gera o backup do diretório de dados `<data_root>/<app_id>` (vazio se ainda não existe).
pub fn export(
    data_root: &Path,
    app_id: &str,
    app_version: &str,
    now_secs: u64,
) -> Result<Vec<u8>, String> {
    if !superapp_package::valid_app_id(app_id) {
        return Err("id de mini-app inválido".into());
    }
    let dir = data_root.join(app_id);
    let mut entries = Vec::new();
    if dir.is_dir() {
        for f in std::fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            // O nome do arquivo é a chave em hex (ver `DirStorage`); o resto é ignorado.
            let Some(key) = f
                .file_name()
                .to_str()
                .and_then(from_hex)
                .and_then(|b| String::from_utf8(b).ok())
            else {
                continue;
            };
            let value = std::fs::read(f.path()).map_err(|e| e.to_string())?;
            entries.push(Entry {
                key,
                value_hex: to_hex(&value),
            });
        }
    }
    entries.sort_by(|a, b| a.key.cmp(&b.key));
    let b = Backup {
        format: FORMAT.into(),
        app_id: app_id.into(),
        app_version: app_version.into(),
        created_at: now_secs,
        entries,
    };
    serde_json::to_vec_pretty(&b).map_err(|e| e.to_string())
}

/// Restaura `bytes` como os dados de `app_id`, substituindo os atuais. Devolve quantas
/// entradas foram restauradas.
pub fn import(data_root: &Path, app_id: &str, bytes: &[u8]) -> Result<usize, String> {
    if !superapp_package::valid_app_id(app_id) {
        return Err("id de mini-app inválido".into());
    }
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err("backup grande demais".into());
    }
    let b: Backup = serde_json::from_slice(bytes).map_err(|e| format!("backup inválido: {e}"))?;
    if b.format != FORMAT {
        return Err("formato de backup desconhecido".into());
    }
    if b.app_id != app_id {
        return Err(format!(
            "este backup é do mini-app \"{}\", não de \"{app_id}\"",
            b.app_id
        ));
    }
    if b.entries.len() > MAX_ENTRIES {
        return Err("backup com entradas demais".into());
    }
    // Valida tudo antes de escrever qualquer coisa.
    let mut seen = HashSet::new();
    let mut total = 0usize;
    let mut decoded = Vec::with_capacity(b.entries.len());
    for e in &b.entries {
        if e.key.is_empty() || e.key.len() > MAX_KEY_BYTES {
            return Err("chave de tamanho inválido no backup".into());
        }
        if !seen.insert(e.key.clone()) {
            return Err("chave repetida no backup".into());
        }
        let v = from_hex(&e.value_hex).ok_or("valor com hexadecimal inválido no backup")?;
        if v.len() > MAX_VALUE_BYTES {
            return Err("valor grande demais no backup".into());
        }
        total += v.len();
        if total > MAX_TOTAL_BYTES {
            return Err("o backup passa da quota de armazenamento".into());
        }
        decoded.push((e.key.as_str(), v));
    }

    // Prepara ao lado e troca: nomes com ponto inicial nunca colidem com ids válidos.
    let staging = data_root.join(format!(".restore-{app_id}"));
    let old = data_root.join(format!(".old-{app_id}"));
    let live = data_root.join(app_id);
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_dir_all(&old);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let prepared = decoded.iter().try_for_each(|(k, v)| {
        std::fs::write(staging.join(to_hex(k.as_bytes())), v).map_err(|e| e.to_string())
    });
    if let Err(e) = prepared {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    if live.exists() {
        if let Err(e) = std::fs::rename(&live, &old) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e.to_string());
        }
    }
    if let Err(e) = std::fs::rename(&staging, &live) {
        // Desfaz: volta os dados antigos.
        let _ = std::fs::rename(&old, &live);
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e.to_string());
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(decoded.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{DirStorage, Storage};

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("superapp-bk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn backup_json(app: &str, entries: &str) -> Vec<u8> {
        format!(
            r#"{{"format":"{FORMAT}","app_id":"{app}","app_version":"1.0.0","created_at":1,"entries":{entries}}}"#
        )
        .into_bytes()
    }

    #[test]
    fn export_then_import_roundtrips_including_odd_keys_and_binary_values() {
        let root = tmp("roundtrip");
        let s = DirStorage::open(&root, "notas").unwrap();
        s.set("tarefas", b"pao\nleite").unwrap();
        s.set("chave/estranha com espaço ../x", &[0, 255, 1, 2])
            .unwrap();
        let bytes = export(&root, "notas", "1.2.0", 42).unwrap();

        // Apaga e restaura em outra raiz.
        let root2 = tmp("roundtrip2");
        let n = import(&root2, "notas", &bytes).unwrap();
        assert_eq!(n, 2);
        let s2 = DirStorage::open(&root2, "notas").unwrap();
        assert_eq!(s2.get("tarefas").unwrap().unwrap(), b"pao\nleite");
        assert_eq!(
            s2.get("chave/estranha com espaço ../x").unwrap().unwrap(),
            vec![0, 255, 1, 2]
        );
    }

    #[test]
    fn import_replaces_existing_data_instead_of_merging() {
        let root = tmp("replace");
        let s = DirStorage::open(&root, "notas").unwrap();
        s.set("velha", b"x").unwrap();
        s.set("comum", b"antes").unwrap();
        import(
            &root,
            "notas",
            &backup_json("notas", r#"[{"key":"comum","value_hex":"646570"}]"#),
        )
        .unwrap();
        assert_eq!(s.get("velha").unwrap(), None);
        assert_eq!(s.get("comum").unwrap().unwrap(), b"dep");
        // Não sobram diretórios temporários.
        assert!(!root.join(".restore-notas").exists());
        assert!(!root.join(".old-notas").exists());
    }

    #[test]
    fn export_of_an_app_without_data_is_an_empty_backup() {
        let root = tmp("empty");
        let bytes = export(&root, "novo", "1.0.0", 1).unwrap();
        assert_eq!(import(&tmp("empty2"), "novo", &bytes).unwrap(), 0);
    }

    #[test]
    fn bad_backups_are_refused_and_leave_data_untouched() {
        let root = tmp("bad");
        let s = DirStorage::open(&root, "notas").unwrap();
        s.set("k", b"intacto").unwrap();
        let big = "00".repeat(MAX_VALUE_BYTES + 1);
        let one_mib = "00".repeat(MAX_VALUE_BYTES);
        let quota = format!(
            "[{}]",
            (0..5)
                .map(|i| format!(r#"{{"key":"k{i}","value_hex":"{one_mib}"}}"#))
                .collect::<Vec<_>>()
                .join(",")
        );
        let long_key = "a".repeat(MAX_KEY_BYTES + 1);
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("não é json", b"lixo".to_vec()),
            (
                "outro app",
                backup_json("outro", r#"[]"#),
            ),
            (
                "formato",
                br#"{"format":"x","app_id":"notas","app_version":"1","created_at":1,"entries":[]}"#
                    .to_vec(),
            ),
            (
                "campo extra",
                br#"{"format":"superapp-backup-1","app_id":"notas","app_version":"1","created_at":1,"entries":[],"x":1}"#
                    .to_vec(),
            ),
            (
                "chave repetida",
                backup_json(
                    "notas",
                    r#"[{"key":"a","value_hex":""},{"key":"a","value_hex":""}]"#,
                ),
            ),
            (
                "chave vazia",
                backup_json("notas", r#"[{"key":"","value_hex":""}]"#),
            ),
            (
                "chave longa",
                backup_json(
                    "notas",
                    &format!(r#"[{{"key":"{long_key}","value_hex":""}}]"#),
                ),
            ),
            (
                "hex ímpar",
                backup_json("notas", r#"[{"key":"a","value_hex":"abc"}]"#),
            ),
            (
                "hex inválido",
                backup_json("notas", r#"[{"key":"a","value_hex":"zz"}]"#),
            ),
            (
                "valor grande",
                backup_json(
                    "notas",
                    &format!(r#"[{{"key":"a","value_hex":"{big}"}}]"#),
                ),
            ),
            ("quota", backup_json("notas", &quota)),
        ];
        for (name, bytes) in cases {
            assert!(
                import(&root, "notas", &bytes).is_err(),
                "deveria recusar: {name}"
            );
            assert_eq!(
                s.get("k").unwrap().unwrap(),
                b"intacto",
                "dados alterados por backup ruim: {name}"
            );
        }
        // Id de app inválido nunca vira caminho.
        assert!(import(&root, "../x", b"{}").is_err());
        assert!(export(&root, "../x", "1", 1).is_err());
    }

    #[test]
    fn oversize_files_are_refused_before_parsing() {
        let root = tmp("oversize");
        assert!(import(&root, "notas", &vec![b' '; MAX_BACKUP_BYTES + 1]).is_err());
    }
}
