//! Armazenamento chave-valor por mini-app: isolado, com chave/valor/quota limitados.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub const MAX_KEY_BYTES: usize = 100;
pub const MAX_VALUE_BYTES: usize = 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;

pub trait Storage: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, String>;
    fn set(&self, key: &str, value: &[u8]) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

fn check_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        return Err(format!("chave deve ter 1..={MAX_KEY_BYTES} bytes"));
    }
    Ok(())
}

fn check_value(value: &[u8]) -> Result<(), String> {
    if value.len() > MAX_VALUE_BYTES {
        return Err(format!("valor acima de {MAX_VALUE_BYTES} bytes"));
    }
    Ok(())
}

/// Em memória (testes e mini-apps efêmeros).
#[derive(Default)]
pub struct MemoryStorage {
    map: Mutex<HashMap<String, Vec<u8>>>,
}

impl Storage for MemoryStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        check_key(key)?;
        Ok(self.map.lock().expect("storage").get(key).cloned())
    }
    fn set(&self, key: &str, value: &[u8]) -> Result<(), String> {
        check_key(key)?;
        check_value(value)?;
        let mut m = self.map.lock().expect("storage");
        let others: usize = m
            .iter()
            .filter(|(k, _)| k.as_str() != key)
            .map(|(k, v)| k.len() + v.len())
            .sum();
        if others + key.len() + value.len() > MAX_TOTAL_BYTES {
            return Err("quota de armazenamento excedida".into());
        }
        m.insert(key.to_owned(), value.to_vec());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        check_key(key)?;
        self.map.lock().expect("storage").remove(key);
        Ok(())
    }
}

/// Em disco: um diretório por mini-app. O nome do arquivo é a chave em hexadecimal, então
/// nenhuma chave consegue sair do diretório (`..`, `/`, `\`, nomes reservados do Windows).
pub struct DirStorage {
    dir: PathBuf,
}

/// Id de mini-app aceito como nome de diretório: `[a-z0-9-]{1,32}` (definido no pacote).
pub use superapp_package::valid_app_id;

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

impl DirStorage {
    pub fn open(root: impl Into<PathBuf>, app_id: &str) -> Result<Self, String> {
        if !valid_app_id(app_id) {
            return Err("id de mini-app inválido".into());
        }
        let dir = root.into().join(app_id);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Self { dir })
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(hex(key))
    }

    fn total_except(&self, key_file: &PathBuf) -> usize {
        std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| &e.path() != key_file)
                    .filter_map(|e| e.metadata().ok())
                    .map(|m| m.len() as usize)
                    .sum()
            })
            .unwrap_or(0)
    }
}

impl Storage for DirStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        check_key(key)?;
        match std::fs::read(self.path(key)) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn set(&self, key: &str, value: &[u8]) -> Result<(), String> {
        check_key(key)?;
        check_value(value)?;
        let p = self.path(key);
        if self.total_except(&p) + value.len() > MAX_TOTAL_BYTES {
            return Err("quota de armazenamento excedida".into());
        }
        std::fs::write(p, value).map_err(|e| e.to_string())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        check_key(key)?;
        match std::fs::remove_file(self.path(key)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}
