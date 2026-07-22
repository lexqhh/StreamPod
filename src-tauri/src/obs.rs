//! Détection de l'installation OBS Studio sur la machine.

use serde::Serialize;
use std::path::PathBuf;

/// Nom du processus principal d'OBS sur Windows 64 bits.
const OBS_PROCESS_NAMES: &[&str] = &["obs64.exe", "obs32.exe", "obs.exe"];

#[derive(Debug, Clone, Serialize)]
pub struct ObsInfo {
    /// Dossier de configuration (%APPDATA%\obs-studio), s'il existe.
    pub config_dir: Option<PathBuf>,
    /// Dossier d'installation (C:\Program Files\obs-studio), s'il existe.
    pub install_dir: Option<PathBuf>,
    /// Version d'OBS installée (ex. "31.0.2"), si détectable.
    pub version: Option<String>,
    /// OBS est-il en cours d'exécution ?
    pub running: bool,
}

/// Dossier de configuration OBS. Surchargeable via la variable
/// d'environnement STREAMPOD_CONFIG_DIR (utilisée par les tests end-to-end).
pub fn config_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("STREAMPOD_CONFIG_DIR") {
        let p = PathBuf::from(dir);
        return p.exists().then_some(p);
    }
    let p = dirs::config_dir()?.join("obs-studio");
    p.exists().then_some(p)
}

/// Dossier de configuration cible pour une restauration : contrairement à
/// `config_dir()`, il est retourné même s'il n'existe pas encore (OBS
/// fraîchement installé, jamais lancé).
pub fn config_dir_target() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("STREAMPOD_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    Some(dirs::config_dir()?.join("obs-studio"))
}

/// Dossier où sont déposés les assets restaurés. Surchargeable via
/// STREAMPOD_ASSETS_DIR (tests).
pub fn assets_target_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("STREAMPOD_ASSETS_DIR") {
        return Some(PathBuf::from(dir));
    }
    Some(dirs::document_dir()?.join("OBS-Backup-Assets"))
}

/// Dossier d'installation OBS. Surchargeable via STREAMPOD_INSTALL_DIR (tests).
pub fn install_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("STREAMPOD_INSTALL_DIR") {
        let p = PathBuf::from(dir);
        return p.exists().then_some(p);
    }
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        if let Ok(key) = hklm.open_subkey("SOFTWARE\\OBS Studio") {
            if let Ok(path) = key.get_value::<String, _>("") {
                let p = PathBuf::from(path);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    for candidate in [
        "C:\\Program Files\\obs-studio",
        "C:\\Program Files (x86)\\obs-studio",
    ] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Version d'OBS installée, lue dans le registre Windows.
/// Surchargeable via STREAMPOD_OBS_VERSION (tests).
pub fn installed_version() -> Option<String> {
    if let Ok(v) = std::env::var("STREAMPOD_OBS_VERSION") {
        return Some(v);
    }
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        use winreg::RegKey;
        // L'installateur OBS écrit sa clé de désinstallation sous WOW6432Node.
        const UNINSTALL_KEYS: &[&str] = &[
            "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\OBS Studio",
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\OBS Studio",
        ];
        for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            let root = RegKey::predef(hive);
            for key_path in UNINSTALL_KEYS {
                if let Ok(key) = root.open_subkey(key_path) {
                    if let Ok(v) = key.get_value::<String, _>("DisplayVersion") {
                        if !v.is_empty() {
                            return Some(v);
                        }
                    }
                }
            }
        }
    }
    None
}

/// OBS est-il en train de tourner ?
pub fn is_running() -> bool {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.processes().values().any(|p| {
        let name = p.name().to_string_lossy().to_lowercase();
        OBS_PROCESS_NAMES.contains(&name.as_str())
    })
}

pub fn detect() -> ObsInfo {
    ObsInfo {
        config_dir: config_dir(),
        install_dir: install_dir(),
        version: installed_version(),
        running: is_running(),
    }
}
