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

/// Dossier des plugins tiers installés hors du dossier d'OBS
/// (%ProgramData%\obs-studio\plugins), s'il existe.
/// Surchargeable via STREAMPOD_PLUGINS_DIR (tests).
pub fn plugins_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("STREAMPOD_PLUGINS_DIR") {
        let p = PathBuf::from(dir);
        return p.exists().then_some(p);
    }
    // Emplacement recommandé par OBS (depuis la 30) pour les installations
    // manuelles ; le crate `dirs` ne l'expose pas.
    let p = PathBuf::from(std::env::var_os("ProgramData")?)
        .join("obs-studio")
        .join("plugins");
    p.is_dir().then_some(p)
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
    process_actif(OBS_PROCESS_NAMES)
}

/// L'un de ces noms d'exécutable correspond-il à un processus vivant ?
/// `noms` doit être en minuscules ; la comparaison ignore la casse.
#[cfg(windows)]
fn process_actif(noms: &[&str]) -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    /// Referme le snapshot quelle que soit la façon dont la boucle se termine.
    struct Snapshot(HANDLE);

    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    unsafe {
        // Un seul appel noyau liste tous les processus : les noms sont ensuite
        // lus en mémoire, sans ouvrir le moindre handle de processus.
        let Ok(handle) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return false;
        };
        let snapshot = Snapshot(handle);

        let mut entry = PROCESSENTRY32W {
            // Process32FirstW échoue si la taille n'est pas renseignée.
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot.0, &mut entry).is_err() {
            return false;
        }
        loop {
            let fin = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let image = &entry.szExeFile[..fin];
            if noms.iter().any(|attendu| nom_egal(image, attendu)) {
                return true;
            }
            if Process32NextW(snapshot.0, &mut entry).is_err() {
                return false;
            }
        }
    }
}

/// Un nom d'image Windows (UTF-16, terminateur exclu) est-il celui attendu ?
/// Comparaison directe, sans allocation : construire une `String` par
/// processus coûtait autant que l'énumération elle-même. `attendu` est un nom
/// de fichier ASCII en minuscules, la casse de l'image est ignorée.
#[cfg(windows)]
fn nom_egal(image: &[u16], attendu: &str) -> bool {
    image.len() == attendu.len()
        && image
            .iter()
            .zip(attendu.bytes())
            .all(|(&c, b)| c < 128 && (c as u8).to_ascii_lowercase() == b)
}

#[cfg(not(windows))]
fn process_actif(noms: &[&str]) -> bool {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.processes().values().any(|p| {
        let name = p.name().to_string_lossy().to_lowercase();
        noms.contains(&name.as_str())
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

#[cfg(test)]
mod tests {
    use super::process_actif;

    /// L'énumération est vérifiée sans dépendre d'un OBS installé : le binaire
    /// de test lui-même est forcément vivant, un nom inventé ne l'est pas.
    #[test]
    fn process_actif_reconnait_le_binaire_de_test() {
        let exe = std::env::current_exe().expect("chemin de l'exécutable de test");
        let nom = exe
            .file_name()
            .expect("nom de fichier")
            .to_string_lossy()
            .to_lowercase();

        assert!(process_actif(&[nom.as_str()]));
        assert!(!process_actif(&["streampod-processus-inexistant.exe"]));
    }
}
