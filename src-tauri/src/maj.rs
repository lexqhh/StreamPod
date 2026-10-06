//! Mise à jour intégrée : détection du mode d'exécution, réglage « vérifier au
//! démarrage », vérification de signature et remplacement de l'exécutable
//! portable. Le téléchargement et l'installation NSIS passent par
//! `tauri-plugin-updater` (lib.rs) ; tout ce qui touche au disque est ici, sur
//! des chemins injectés pour être testable sans exe réel.

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Cible de `latest.json` pour l'exécutable portable (l'installateur utilise
/// la cible par défaut du plugin, `windows-x86_64`).
pub const CIBLE_PORTABLE: &str = "windows-x86_64-portable";

/// Argument passé au nouvel exe portable relancé : il attend la fin de
/// l'ancien processus avant de démarrer (instance unique).
pub const ARG_APRES_MAJ: &str = "--apres-maj";

const FICHIER_REGLAGE: &str = "mise-a-jour.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModeExecution {
    Installe,
    Portable,
}

/// Installé si l'installateur NSIS a déposé `uninstall.exe` à côté de l'exe ou
/// si la clé `Uninstall` du registre désigne ce dossier ; portable sinon.
pub fn mode_depuis(dossier_exe: &Path, install_location: Option<&str>) -> ModeExecution {
    let dans_registre = install_location.is_some_and(|l| {
        let l = l.trim().trim_matches('"').trim_end_matches(['\\', '/']);
        !l.is_empty()
            && crate::scenes::normalize_path(l)
                == crate::scenes::normalize_path(&dossier_exe.to_string_lossy())
    });
    if dans_registre || dossier_exe.join("uninstall.exe").is_file() {
        ModeExecution::Installe
    } else {
        ModeExecution::Portable
    }
}

#[cfg(windows)]
fn install_location() -> Option<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    const CLE: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\StreamPod";
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .iter()
        .find_map(|racine| {
            RegKey::predef(*racine)
                .open_subkey(CLE)
                .and_then(|k| k.get_value::<String, _>("InstallLocation"))
                .ok()
        })
}

#[cfg(not(windows))]
fn install_location() -> Option<String> {
    None
}

pub fn mode_execution(exe: &Path) -> ModeExecution {
    let dossier = exe.parent().unwrap_or(Path::new("."));
    mode_depuis(dossier, install_location().as_deref())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReglageMaj {
    #[serde(default = "vrai")]
    pub verifier_au_demarrage: bool,
}

fn vrai() -> bool {
    true
}

impl Default for ReglageMaj {
    fn default() -> Self {
        Self {
            verifier_au_demarrage: true,
        }
    }
}

/// Réglage absent ou illisible : valeur par défaut (vérification activée).
pub fn lire_reglage(dossier: &Path) -> ReglageMaj {
    fs::read_to_string(dossier.join(FICHIER_REGLAGE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn ecrire_reglage(dossier: &Path, reglage: &ReglageMaj) -> Result<(), String> {
    let err = |e: io::Error| format!("Impossible d'enregistrer le réglage de mise à jour : {e}");
    fs::create_dir_all(dossier).map_err(err)?;
    let json = serde_json::to_string_pretty(reglage).map_err(|e| e.to_string())?;
    fs::write(dossier.join(FICHIER_REGLAGE), json).map_err(err)
}

/// `STREAMPOD_UPDATER_DESACTIVE` non vide : aucune requête (tests, CI).
pub fn desactivee_par_env() -> bool {
    std::env::var_os("STREAMPOD_UPDATER_DESACTIVE").is_some_and(|v| !v.is_empty())
}

/// Vérifie `octets` contre une signature et une clé publique au format Tauri
/// (texte minisign encodé en base64).
pub fn verifier_signature(
    octets: &[u8],
    signature: &str,
    cle_publique: &str,
) -> Result<(), String> {
    const MSG: &str = "Signature de la mise à jour invalide : fichier refusé.";
    let b64 = |s: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .ok()
            .and_then(|o| String::from_utf8(o).ok())
            .ok_or(MSG)
    };
    let cle = minisign_verify::PublicKey::decode(&b64(cle_publique)?).map_err(|_| MSG)?;
    let sig = minisign_verify::Signature::decode(&b64(signature)?).map_err(|_| MSG)?;
    cle.verify(octets, &sig, true).map_err(|_| MSG.to_string())
}

fn avec_suffixe(exe: &Path, suffixe: &str) -> PathBuf {
    let mut nom = exe.as_os_str().to_os_string();
    nom.push(suffixe);
    PathBuf::from(nom)
}

pub fn chemin_ancien(exe: &Path) -> PathBuf {
    avec_suffixe(exe, ".old")
}

/// Sonde d'écriture réelle (clé USB en lecture seule, dossier protégé) : la
/// création d'un fichier témoin est le seul test fiable sous Windows.
pub fn dossier_inscriptible(dossier: &Path) -> bool {
    let temoin = dossier.join(".streampod-maj-test");
    let ok = fs::write(&temoin, b"").is_ok();
    let _ = fs::remove_file(&temoin);
    ok
}

pub const MSG_LECTURE_SEULE: &str =
    "Le dossier de StreamPod n'est pas accessible en écriture (clé USB protégée ou dossier système) : \
     téléchargez la nouvelle version depuis la page de la release.";

/// Signature vérifiée puis remplacement de `exe` par `octets`.
pub fn installer_portable(
    exe: &Path,
    octets: &[u8],
    signature: &str,
    cle_publique: &str,
) -> Result<(), String> {
    verifier_signature(octets, signature, cle_publique)?;
    remplacer_exe(exe, octets, |de, vers| fs::rename(de, vers))
}

/// `.new` écrit à côté, exe courant → `.old`, `.new` → exe. Jamais d'état sans
/// exécutable : si le 2ᵉ renommage échoue, `.old` est remis en place.
/// `renommer` est injectable pour simuler les échecs en test.
pub fn remplacer_exe(
    exe: &Path,
    octets: &[u8],
    mut renommer: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<(), String> {
    let dossier = exe.parent().unwrap_or(Path::new("."));
    if !dossier_inscriptible(dossier) {
        return Err(MSG_LECTURE_SEULE.to_string());
    }
    let nouveau = avec_suffixe(exe, ".new");
    let ancien = chemin_ancien(exe);
    let echec = |etape: &str, e: io::Error| format!("Mise à jour impossible ({etape}) : {e}");

    fs::write(&nouveau, octets).map_err(|e| echec("écriture de la nouvelle version", e))?;
    let _ = fs::remove_file(&ancien);
    if let Err(e) = renommer(exe, &ancien) {
        let _ = fs::remove_file(&nouveau);
        return Err(echec("mise de côté de la version actuelle", e));
    }
    if let Err(e) = renommer(&nouveau, exe) {
        let _ = fs::remove_file(&nouveau);
        return match renommer(&ancien, exe) {
            Ok(()) => Err(echec("mise en place", e)),
            Err(e2) => Err(format!(
                "Mise à jour impossible et restauration échouée ({e2}) : renommez « {} » en « {} » pour retrouver StreamPod.",
                ancien.display(),
                exe.display()
            )),
        };
    }
    Ok(())
}

/// Supprime `<exe>.old` laissé par une mise à jour. Après une relance
/// (`attendre`), réessaie le temps que l'ancien processus libère son image.
pub fn nettoyer_ancien(exe: &Path, attendre: bool) {
    let ancien = chemin_ancien(exe);
    let essais = if attendre { 50 } else { 1 };
    for i in 0..essais {
        if !ancien.exists() || fs::remove_file(&ancien).is_ok() {
            return;
        }
        if i + 1 < essais {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_portable_sans_desinstalleur_ni_registre() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(mode_depuis(dir.path(), None), ModeExecution::Portable);
        assert_eq!(
            mode_depuis(dir.path(), Some(r#""C:\Ailleurs\StreamPod""#)),
            ModeExecution::Portable
        );
    }

    #[test]
    fn mode_installe_par_desinstalleur_ou_registre() {
        let dir = tempfile::tempdir().unwrap();
        let loc = format!("\"{}\\\"", dir.path().display());
        assert_eq!(mode_depuis(dir.path(), Some(&loc)), ModeExecution::Installe);
        fs::write(dir.path().join("uninstall.exe"), b"").unwrap();
        assert_eq!(mode_depuis(dir.path(), None), ModeExecution::Installe);
    }

    #[test]
    fn reglage_active_par_defaut_et_persiste() {
        let dir = tempfile::tempdir().unwrap();
        let sous = dir.path().join("com.streampod.app");
        assert!(lire_reglage(&sous).verifier_au_demarrage);
        ecrire_reglage(
            &sous,
            &ReglageMaj {
                verifier_au_demarrage: false,
            },
        )
        .unwrap();
        assert!(!lire_reglage(&sous).verifier_au_demarrage);
        fs::write(sous.join(FICHIER_REGLAGE), "illisible").unwrap();
        assert!(lire_reglage(&sous).verifier_au_demarrage);
    }

    #[test]
    fn variable_d_environnement_desactive_la_verification() {
        std::env::set_var("STREAMPOD_UPDATER_DESACTIVE", "1");
        assert!(desactivee_par_env());
        std::env::set_var("STREAMPOD_UPDATER_DESACTIVE", "");
        assert!(!desactivee_par_env());
        std::env::remove_var("STREAMPOD_UPDATER_DESACTIVE");
        assert!(!desactivee_par_env());
    }
}
