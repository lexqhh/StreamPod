//! Pipeline de restauration : fichier .obsbackup → configuration OBS.

use crate::backup::{asset_mapping_from_manifest, Manifest, Progress};
use crate::{obs, scenes};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

fn err(context: &str, e: impl std::fmt::Display) -> String {
    format!("{context} : {e}")
}

fn chemin_suspect(rel: &str) -> String {
    format!("Archive invalide ou malveillante : chemin suspect \"{rel}\".")
}

/// Joint un chemin relatif issu de l'archive (séparé par des `/`) sous `base`,
/// en n'acceptant que des composants de chemin normaux.
///
/// Protection contre l'écriture de fichier arbitraire (zip slip) : sans cette
/// validation, `PathBuf::join` abandonne `base` dès qu'un composant est absolu
/// (préfixe de lecteur Windows `C:\…`), et une archive piégée pourrait écrire
/// n'importe où sur le disque. S'applique aussi bien aux noms d'entrées ZIP
/// qu'aux chemins lus depuis le manifest, tous deux contrôlés par l'auteur de
/// l'archive.
fn chemin_relatif_sur(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let suspect = || chemin_suspect(rel);
    let mut out = base.to_path_buf();
    for comp in rel.split('/') {
        if comp.is_empty()
            || comp == "."
            || comp == ".."
            || comp.contains('\\')
            || comp.contains(':')
            || comp.chars().any(char::is_control)
        {
            return Err(suspect());
        }
        // Ceinture et bretelles : une fois interprété par le système, le
        // composant doit rester exactement un unique composant normal (ni
        // préfixe de lecteur, ni racine, ni séparateur qui aurait échappé
        // aux vérifications ci-dessus).
        let mut parts = Path::new(comp).components();
        match (parts.next(), parts.next()) {
            (Some(std::path::Component::Normal(_)), None) => out.push(comp),
            _ => return Err(suspect()),
        }
    }
    Ok(out)
}

/// Résumé présenté à l'utilisateur avant de lancer la restauration.
#[derive(Debug, Clone, Serialize)]
pub struct RestorePreview {
    pub manifest: Manifest,
    pub backup_file_size: u64,
    pub obs_installed: bool,
    pub installed_version: Option<String>,
    /// Les plugins peuvent-ils être copiés automatiquement ?
    pub plugins_compatible: bool,
    /// Une configuration OBS existe déjà et sera remplacée (après copie de
    /// sécurité automatique).
    pub config_exists: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreSummary {
    pub scene_collections: usize,
    pub profiles: usize,
    pub assets_restored: usize,
    pub assets_dir: Option<String>,
    /// "copied" | "copied_elevated" | "manual" | "none"
    pub plugins_status: String,
    pub plugins: Vec<String>,
    /// Copie de sécurité de l'ancienne configuration, le cas échéant.
    pub previous_config_backup: Option<String>,
}

fn open_archive(backup_path: &Path) -> Result<ZipArchive<File>, String> {
    let file = File::open(backup_path)
        .map_err(|e| err(&format!("Ouverture de {}", backup_path.display()), e))?;
    ZipArchive::new(file).map_err(|e| {
        err(
            "Ce fichier n'est pas une sauvegarde .obsbackup valide",
            e,
        )
    })
}

fn read_manifest(archive: &mut ZipArchive<File>) -> Result<Manifest, String> {
    let mut entry = archive
        .by_name("manifest.json")
        .map_err(|_| "Sauvegarde invalide : manifest.json manquant.".to_string())?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| err("Lecture du manifest", e))?;
    serde_json::from_str(&text).map_err(|e| err("Manifest illisible", e))
}

/// Version majeure ("31.0.2" → "31").
fn major(version: &str) -> &str {
    version.split('.').next().unwrap_or(version)
}

/// Lit une sauvegarde et prépare le résumé avant restauration.
pub fn preview(backup_path: &Path) -> Result<RestorePreview, String> {
    let mut archive = open_archive(backup_path)?;
    let manifest = read_manifest(&mut archive)?;

    let install_dir = obs::install_dir();
    let installed_version = obs::installed_version();
    let obs_installed = install_dir.is_some();

    let plugins_compatible = match (&manifest.obs_version, &installed_version) {
        (Some(a), Some(b)) => major(a) == major(b),
        // Version inconnue d'un côté ou de l'autre : on ne copie pas les
        // DLL automatiquement, par prudence.
        _ => false,
    };

    let config_exists = obs::config_dir().is_some();

    let mut warnings = Vec::new();
    if !obs_installed {
        warnings.push(
            "OBS Studio ne semble pas installé sur cet ordinateur. Installez-le d'abord \
             (obsproject.com), puis relancez la restauration."
                .to_string(),
        );
    }
    if !manifest.plugins.is_empty() && !plugins_compatible {
        warnings.push(format!(
            "La sauvegarde vient d'OBS {} et cet ordinateur a OBS {}. Les {} plugin(s) ne \
             seront pas copiés automatiquement : ils seront listés pour une réinstallation \
             manuelle.",
            manifest.obs_version.as_deref().unwrap_or("?"),
            installed_version.as_deref().unwrap_or("?"),
            manifest.plugins.len()
        ));
    }
    if config_exists {
        warnings.push(
            "Une configuration OBS existe déjà sur cet ordinateur. Elle sera remplacée, \
             mais une copie de sécurité sera conservée automatiquement."
                .to_string(),
        );
    }

    Ok(RestorePreview {
        manifest,
        backup_file_size: backup_path.metadata().map(|m| m.len()).unwrap_or(0),
        obs_installed,
        installed_version,
        plugins_compatible,
        config_exists,
        warnings,
    })
}

/// Extrait une entrée du ZIP vers un fichier sur le disque.
fn extract_entry(
    archive: &mut ZipArchive<File>,
    index: usize,
    dest: &Path,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| err(&format!("Création du dossier {}", parent.display()), e))?;
    }
    let mut entry = archive
        .by_index(index)
        .map_err(|e| err("Lecture de l'archive", e))?;
    let mut out = File::create(dest)
        .map_err(|e| err(&format!("Création de {}", dest.display()), e))?;
    std::io::copy(&mut entry, &mut out)
        .map_err(|e| err(&format!("Extraction vers {}", dest.display()), e))?;
    Ok(())
}

/// Copie récursive (pour les plugins).
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.map_err(std::io::Error::other)?;
        let rel = entry.path().strip_prefix(src).map_err(std::io::Error::other)?;
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Copie les plugins avec élévation (UAC) via PowerShell, quand l'écriture
/// directe dans Program Files est refusée.
fn copy_plugins_elevated(staging: &Path, install_dir: &Path) -> Result<(), String> {
    let script = format!(
        "Copy-Item -Path '{}\\*' -Destination '{}' -Recurse -Force",
        staging.display(),
        install_dir.display()
    );
    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "Start-Process -FilePath powershell -Verb RunAs -Wait -WindowStyle Hidden \
                 -ArgumentList '-NoProfile','-Command',\"{script}\""
            ),
        ])
        .status()
        .map_err(|e| err("Lancement de la copie avec droits administrateur", e))?;
    if status.success() {
        Ok(())
    } else {
        Err("La copie avec droits administrateur a été refusée ou a échoué.".to_string())
    }
}

/// Restaure une sauvegarde .obsbackup.
pub fn restore(
    backup_path: &Path,
    progress: impl Fn(Progress),
) -> Result<RestoreSummary, String> {
    let report = |step: &str, message: String, current: u64, total: u64| {
        progress(Progress {
            step: step.to_string(),
            message,
            current,
            total,
        });
    };

    if obs::is_running() {
        return Err(
            "OBS est en cours d'exécution. Fermez OBS avant de restaurer la sauvegarde."
                .to_string(),
        );
    }

    let mut archive = open_archive(backup_path)?;
    let manifest = read_manifest(&mut archive)?;

    let target_config = obs::config_dir_target()
        .ok_or("Impossible de déterminer le dossier de configuration OBS.")?;
    let parent = target_config
        .parent()
        .ok_or("Dossier de configuration invalide.")?
        .to_path_buf();
    std::fs::create_dir_all(&parent)
        .map_err(|e| err("Préparation du dossier de configuration", e))?;

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let tmp_config = parent.join(format!("obs-studio.tmp-{stamp}"));

    // 1. Extraction de la configuration vers un dossier temporaire (jamais
    //    directement sur la config existante : écriture atomique).
    // 2. Extraction des assets vers le dossier d'assets.
    let assets_dir = obs::assets_target_dir();
    let mut asset_dest_by_archive_path: BTreeMap<String, PathBuf> = BTreeMap::new();
    if !manifest.assets.is_empty() {
        let assets_dir = assets_dir
            .as_ref()
            .ok_or("Impossible de déterminer le dossier Documents pour les assets.")?;
        for asset in &manifest.assets {
            // archive_path = assets/<n>/<nom> → <assets_dir>/<n>/<nom>.
            // Chemin lu depuis le manifest, donc contrôlé par l'auteur de
            // l'archive : préfixe `assets/` obligatoire, puis même validation
            // que les entrées ZIP.
            let rel = asset
                .archive_path
                .strip_prefix("assets/")
                .ok_or_else(|| chemin_suspect(&asset.archive_path))?;
            asset_dest_by_archive_path
                .insert(asset.archive_path.clone(), chemin_relatif_sur(assets_dir, rel)?);
        }
    }

    let total_entries = archive.len() as u64;
    let mut assets_restored = 0usize;
    let mut plugin_staging: Option<PathBuf> = None;

    // Politique d'extraction : une entrée hors des préfixes connus (config/,
    // assets/ du manifest, plugins/64bit/, plugins/data/) est ignorée sans
    // erreur — compatibilité ascendante avec de futurs formats, rien n'est
    // écrit. En revanche, un chemin suspect SOUS un préfixe connu fait
    // échouer toute la restauration (zip slip).
    for i in 0..archive.len() {
        let (name, is_dir) = {
            let entry = archive.by_index(i).map_err(|e| err("Lecture de l'archive", e))?;
            (entry.name().to_string(), entry.is_dir())
        };
        if is_dir {
            continue;
        }
        report("extract", format!("Extraction : {name}"), i as u64, total_entries);

        if let Some(rel) = name.strip_prefix("config/") {
            let dest = chemin_relatif_sur(&tmp_config, rel)?;
            extract_entry(&mut archive, i, &dest)?;
        } else if name.starts_with("assets/") {
            if let Some(dest) = asset_dest_by_archive_path.get(&name) {
                extract_entry(&mut archive, i, dest)?;
                assets_restored += 1;
            }
        } else if let Some(rel) = name.strip_prefix("plugins/") {
            // Extraits d'abord vers un dossier de transit ; copiés vers le
            // dossier d'OBS à l'étape suivante.
            let staging = plugin_staging
                .get_or_insert_with(|| std::env::temp_dir().join(format!("owbs-plugins-{stamp}")));
            // plugins/64bit/x.dll → obs-plugins/64bit/x.dll
            // plugins/data/<stem>/... → data/obs-plugins/<stem>/...
            let dest = if let Some(r) = rel.strip_prefix("64bit/") {
                chemin_relatif_sur(&staging.join("obs-plugins").join("64bit"), r)?
            } else if let Some(r) = rel.strip_prefix("data/") {
                chemin_relatif_sur(&staging.join("data").join("obs-plugins"), r)?
            } else {
                continue;
            };
            extract_entry(&mut archive, i, &dest)?;
        }
    }

    // 3. Réécriture des chemins d'assets dans les scènes extraites.
    report("rewrite", "Mise à jour des chemins d'assets…".into(), 0, 1);
    let mut mapping: BTreeMap<String, String> = BTreeMap::new();
    for (original_normalized, archive_path) in asset_mapping_from_manifest(&manifest) {
        if let Some(dest) = asset_dest_by_archive_path.get(&archive_path) {
            mapping.insert(
                original_normalized,
                dest.to_string_lossy().replace('\\', "/"),
            );
        }
    }
    let scenes_dir = tmp_config.join("basic").join("scenes");
    if scenes_dir.is_dir() && !mapping.is_empty() {
        for entry in std::fs::read_dir(&scenes_dir)
            .map_err(|e| err("Lecture des scènes restaurées", e))?
            .flatten()
        {
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .map_err(|e| err(&format!("Lecture de {}", path.display()), e))?;
            let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            scenes::rewrite_asset_paths(&mut value, &mapping);
            std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap())
                .map_err(|e| err(&format!("Écriture de {}", path.display()), e))?;
        }
    }

    // 4. Bascule : l'ancienne config devient une copie de sécurité, la
    //    nouvelle prend sa place.
    report("swap", "Mise en place de la configuration…".into(), 0, 1);
    let mut previous_backup = None;
    if target_config.exists() {
        let bak = parent.join(format!("obs-studio.bak-{stamp}"));
        std::fs::rename(&target_config, &bak).map_err(|e| {
            err(
                "Impossible de mettre de côté la configuration existante (OBS ouvert ?)",
                e,
            )
        })?;
        previous_backup = Some(bak.to_string_lossy().into_owned());
    }
    std::fs::rename(&tmp_config, &target_config)
        .map_err(|e| err("Mise en place de la nouvelle configuration", e))?;

    // 5. Plugins.
    let mut plugins_status = "none".to_string();
    if !manifest.plugins.is_empty() {
        report("plugins", "Installation des plugins…".into(), 0, 1);
        let install_dir = obs::install_dir();
        let installed_version = obs::installed_version();
        let compatible = match (&manifest.obs_version, &installed_version) {
            (Some(a), Some(b)) => major(a) == major(b),
            _ => false,
        };
        plugins_status = match (install_dir, compatible, plugin_staging.as_ref()) {
            (Some(install), true, Some(staging)) => {
                match copy_dir_recursive(staging, &install) {
                    Ok(()) => "copied".to_string(),
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                        match copy_plugins_elevated(staging, &install) {
                            Ok(()) => "copied_elevated".to_string(),
                            Err(_) => "manual".to_string(),
                        }
                    }
                    Err(_) => "manual".to_string(),
                }
            }
            _ => "manual".to_string(),
        };
    }
    if let Some(staging) = plugin_staging {
        let _ = std::fs::remove_dir_all(staging);
    }

    Ok(RestoreSummary {
        scene_collections: manifest.scene_collections.len(),
        profiles: manifest.profiles.len(),
        assets_restored,
        assets_dir: (assets_restored > 0)
            .then(|| assets_dir.unwrap().to_string_lossy().into_owned()),
        plugins_status,
        plugins: manifest.plugins.iter().map(|p| p.name.clone()).collect(),
        previous_config_backup: previous_backup,
    })
}

#[cfg(test)]
mod tests {
    use super::chemin_relatif_sur;
    use std::path::Path;

    #[test]
    fn chemins_legitimes_acceptes() {
        let base = Path::new("base");
        assert_eq!(
            chemin_relatif_sur(base, "a/b/c.json").unwrap(),
            base.join("a").join("b").join("c.json")
        );
        assert!(chemin_relatif_sur(base, "Ma Collection.json").is_ok());
        // Des points au milieu d'un nom de fichier restent légitimes.
        assert!(chemin_relatif_sur(base, "photo..2026.png").is_ok());
    }

    #[test]
    fn chemins_suspects_rejetes() {
        let base = Path::new("base");
        for rel in [
            r"C:\Users\Public\evil.dll",
            "C:/x",
            r"..\x",
            "a/../b",
            "a/b:ads",
            "/etc/x",
            r"\\x",
            "a//b",
            ".",
            "..",
            "",
            "a/fichier\u{0}.txt",
        ] {
            let e = chemin_relatif_sur(base, rel)
                .expect_err(&format!("« {rel} » aurait dû être rejeté"));
            assert!(e.contains("Archive invalide ou malveillante"), "{e}");
        }
    }
}
