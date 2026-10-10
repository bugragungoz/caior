use serde::Deserialize;
use winreg::{enums::*, RegKey};

#[derive(Deserialize)]
pub struct RegistryCheck {
    pub hive: String,
    pub key: String,
    pub value: Option<String>,
    pub min_dword: Option<u32>,
}

fn check_single(c: &RegistryCheck) -> bool {
    let predef = match c.hive.as_str() {
        "HKLM" => HKEY_LOCAL_MACHINE,
        "HKCU" => HKEY_CURRENT_USER,
        "HKCR" => HKEY_CLASSES_ROOT,
        _ => return false,
    };

    let Ok(key) = RegKey::predef(predef).open_subkey_with_flags(&c.key, KEY_READ) else {
        return false;
    };

    match (&c.value, c.min_dword) {
        (Some(v), Some(min)) => key.get_value::<u32, _>(v).map(|n| n >= min).unwrap_or(false),
        (Some(v), None) => {
            let r1: Result<u32, _> = key.get_value(v);
            let r2: Result<String, _> = key.get_value(v);
            r1.is_ok() || r2.is_ok()
        }
        _ => true,
    }
}

#[tauri::command]
pub fn check_registry_batch(checks: Vec<Option<RegistryCheck>>) -> Vec<Option<bool>> {
    checks
        .iter()
        .map(|c| c.as_ref().map(check_single))
        .collect()
}
