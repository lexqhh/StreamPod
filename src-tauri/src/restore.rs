//! Pipeline de restauration : fichier .obsbackup → configuration OBS.

use crate::backup::{asset_mapping_from_manifest, Manifest, Progress};
use crate::{devices, obs, remap, scenes};
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
pub(crate) fn chemin_relatif_sur(base: &Path, rel: &str) -> Result<PathBuf, String> {
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
    /// "manual" (plugins listés pour réinstallation manuelle) | "none".
    /// Les DLL ne sont JAMAIS installées automatiquement : une archive est
    /// une donnée non fiable, et une DLL déposée dans le dossier d'OBS est
    /// du code exécuté au prochain lancement.
    pub plugins_status: String,
    pub plugins: Vec<String>,
    /// Copie de sécurité de l'ancienne configuration, le cas échéant.
    pub previous_config_backup: Option<String>,
    /// Nombre de sources dont le périphérique a été remplacé selon les choix
    /// de l'utilisateur.
    pub sources_remappees: usize,
}

pub(crate) fn open_archive(backup_path: &Path) -> Result<ZipArchive<File>, String> {
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

/// Lit une sauvegarde et prépare le résumé avant restauration.
pub fn preview(backup_path: &Path) -> Result<RestorePreview, String> {
    let mut archive = open_archive(backup_path)?;
    let manifest = read_manifest(&mut archive)?;

    let install_dir = obs::install_dir();
    let installed_version = obs::installed_version();
    let obs_installed = install_dir.is_some();

    let config_exists = obs::config_dir().is_some();

    let mut warnings = Vec::new();
    if !obs_installed {
        warnings.push(
            "OBS Studio ne semble pas installé sur cet ordinateur. Installez-le d'abord \
             (obsproject.com), puis relancez la restauration."
                .to_string(),
        );
    }
    if !manifest.plugins.is_empty() {
        warnings.push(format!(
            "Par sécurité, les {} plugin(s) de la sauvegarde ne seront pas installés \
             automatiquement : ils vous seront listés à la fin pour une réinstallation \
             depuis leurs sites officiels.",
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

/// Bascule la nouvelle configuration (`tmp`) à la place de l'ancienne
/// (`target`), l'ancienne étant mise de côté dans `bak`.
///
/// Deux renames successifs ne sont jamais atomiques ensemble. Si la mise en
/// place de la nouvelle configuration échoue, l'ancienne est remise en place
/// automatiquement (rollback). Si ce rollback échoue aussi, le message
/// d'erreur donne les chemins exacts pour réparer à la main.
///
/// Retourne le chemin du `.bak` créé, ou `None` si aucune configuration
/// n'existait (première restauration).
fn basculer_config(target: &Path, tmp: &Path, bak: &Path) -> Result<Option<String>, String> {
    basculer_config_avec(target, tmp, bak, |from, to| std::fs::rename(from, to))
}

/// Variante à fonction de rename injectable, pour tester les scénarios
/// d'échec de façon déterministe.
fn basculer_config_avec(
    target: &Path,
    tmp: &Path,
    bak: &Path,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<Option<String>, String> {
    let mut previous_backup = None;
    if target.exists() {
        if let Err(e) = rename(target, bak) {
            // Rien n'a encore bougé : la configuration active est intacte et
            // l'extraction temporaire peut être supprimée sans risque.
            let _ = std::fs::remove_dir_all(tmp);
            return Err(err(
                "Impossible de mettre de côté la configuration existante (OBS ouvert ?)",
                e,
            ));
        }
        previous_backup = Some(bak.to_string_lossy().into_owned());
    }
    if let Err(e) = rename(tmp, target) {
        if previous_backup.is_none() {
            // Première restauration : rien à remettre en place.
            let _ = std::fs::remove_dir_all(tmp);
            return Err(err("Mise en place de la nouvelle configuration", e));
        }
        if let Err(rb) = rename(bak, target) {
            return Err(format!(
                "La mise en place de la nouvelle configuration a échoué ({e}), et la remise \
                 en place de votre ancienne configuration a échoué aussi ({rb}). Votre \
                 configuration d'origine est intacte dans « {} » : renommez ce dossier en \
                 « {} » pour la retrouver. La configuration extraite de la sauvegarde se \
                 trouve dans « {} ».",
                bak.display(),
                target.display(),
                tmp.display()
            ));
        }
        // Rollback réussi : ne pas laisser traîner le dossier temporaire.
        let _ = std::fs::remove_dir_all(tmp);
        return Err(format!(
            "La restauration a échoué ({e}). Votre configuration d'origine a été remise en \
             place : la configuration OBS active n'a pas été modifiée."
        ));
    }
    Ok(previous_backup)
}

/// Destination d'un asset : extrait d'abord dans un dossier de transit propre
/// à cette restauration, puis déplacé vers son emplacement définitif une fois
/// toute la préparation réussie — le dossier d'assets définitif n'est jamais
/// touché par une restauration qui échoue en cours de route.
struct DestinationAsset {
    transit: PathBuf,
    finale: PathBuf,
}

/// Déplace les assets du dossier de transit vers leur emplacement définitif.
/// Les fichiers créés (absents auparavant) sont ajoutés à `installes` au fil
/// de l'eau, pour pouvoir les retirer si la suite de la restauration échoue.
///
/// En cas de collision (un asset du même nom au même index existe déjà, par
/// exemple laissé par une restauration précédente), le fichier est écrasé —
/// même comportement qu'avant — et n'est pas retiré en cas de rollback.
fn installer_assets(
    destinations: &BTreeMap<String, DestinationAsset>,
    installes: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for dest in destinations.values() {
        // Entrée listée au manifest mais absente de l'archive : rien à faire.
        if !dest.transit.is_file() {
            continue;
        }
        if let Some(parent) = dest.finale.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| err(&format!("Création du dossier {}", parent.display()), e))?;
        }
        let existait = dest.finale.exists();
        if existait {
            std::fs::remove_file(&dest.finale)
                .map_err(|e| err(&format!("Remplacement de {}", dest.finale.display()), e))?;
        }
        std::fs::rename(&dest.transit, &dest.finale)
            .map_err(|e| err(&format!("Mise en place de {}", dest.finale.display()), e))?;
        if !existait {
            installes.push(dest.finale.clone());
        }
    }
    Ok(())
}

/// Retire (best effort) les assets installés par une restauration qui a
/// échoué ensuite, ainsi que leurs sous-dossiers devenus vides.
fn retirer_assets(installes: &[PathBuf]) {
    for fichier in installes {
        let _ = std::fs::remove_file(fichier);
        if let Some(parent) = fichier.parent() {
            // Ne supprime le dossier que s'il est vide.
            let _ = std::fs::remove_dir(parent);
        }
    }
}

/// Restaure une sauvegarde .obsbackup en appliquant les choix de remappage
/// matériel confirmés par l'utilisateur (`choix` peut être vide).
pub fn restore(
    backup_path: &Path,
    choix: &[remap::Choix],
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

    // Revalidation des choix de remappage contre l'inventaire actuel, avant
    // la moindre écriture : un périphérique débranché depuis l'aperçu arrête
    // tout ici, la configuration en place et le disque restent intacts.
    let choix_valides = if choix.is_empty() {
        Vec::new()
    } else {
        let inventaire = devices::inventaire()?;
        let rapport = remap::analyser(backup_path, inventaire.clone())?;
        remap::valider_choix(choix, &inventaire, &rapport.a_confirmer)?
    };

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
    //    directement sur la config existante : la bascule ne se fait qu'une
    //    fois l'extraction terminée, avec rollback en cas d'échec).
    // 2. Extraction des assets vers un dossier de transit voisin du dossier
    //    définitif (même volume, donc mise en place par simple rename) :
    //    le dossier d'assets définitif n'est touché qu'une fois toute la
    //    préparation réussie.
    let assets_dir = obs::assets_target_dir();
    let mut asset_dest_by_archive_path: BTreeMap<String, DestinationAsset> = BTreeMap::new();
    let mut tmp_assets: Option<PathBuf> = None;
    if !manifest.assets.is_empty() {
        let assets_dir = assets_dir
            .as_ref()
            .ok_or("Impossible de déterminer le dossier Documents pour les assets.")?;
        let transit = assets_dir.with_file_name(format!(
            "{}.tmp-{stamp}",
            assets_dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ));
        for asset in &manifest.assets {
            // archive_path = assets/<n>/<nom> → <assets_dir>/<n>/<nom>.
            // Chemin lu depuis le manifest, donc contrôlé par l'auteur de
            // l'archive : préfixe `assets/` obligatoire, puis même validation
            // que les entrées ZIP.
            let rel = asset
                .archive_path
                .strip_prefix("assets/")
                .ok_or_else(|| chemin_suspect(&asset.archive_path))?;
            asset_dest_by_archive_path.insert(
                asset.archive_path.clone(),
                DestinationAsset {
                    transit: chemin_relatif_sur(&transit, rel)?,
                    finale: chemin_relatif_sur(assets_dir, rel)?,
                },
            );
        }
        tmp_assets = Some(transit);
    }

    // Toute erreur entre l'extraction et la bascule supprime les deux
    // dossiers temporaires : ni configuration partielle, ni assets de
    // transit orphelins — la configuration active et le dossier d'assets
    // définitif n'ont pas bougé.
    let nettoyer_temporaires = || {
        let _ = std::fs::remove_dir_all(&tmp_config);
        if let Some(transit) = &tmp_assets {
            let _ = std::fs::remove_dir_all(transit);
        }
    };

    let total_entries = archive.len() as u64;

    // Politique d'extraction : une entrée hors des préfixes connus (config/,
    // assets/ du manifest) est ignorée sans erreur — compatibilité
    // ascendante avec de futurs formats, rien n'est écrit. En revanche, un
    // chemin suspect SOUS un préfixe connu fait échouer toute la
    // restauration (zip slip).
    //
    // Les entrées plugins/ ne sont JAMAIS extraites : une DLL écrite dans le
    // dossier d'OBS (même via un dossier de transit) serait du code exécuté
    // au prochain lancement, et le manifest qui prétend la rendre
    // « compatible » est écrit par l'auteur de l'archive. Les plugins sont
    // seulement listés (manifest.plugins) pour réinstallation manuelle
    // depuis leurs sites officiels.
    let scenes_dir = tmp_config.join("basic").join("scenes");
    let preparation = (|| -> Result<(usize, usize), String> {
        let mut assets_restored = 0usize;
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
                    extract_entry(&mut archive, i, &dest.transit)?;
                    assets_restored += 1;
                }
            }
        }

        // 3. Réécriture des chemins d'assets dans les scènes extraites, avec
        //    les chemins définitifs (les assets n'y seront déplacés qu'à
        //    l'étape 5).
        report("rewrite", "Mise à jour des chemins d'assets…".into(), 0, 1);
        let mut mapping: BTreeMap<String, String> = BTreeMap::new();
        for (original_normalized, archive_path) in asset_mapping_from_manifest(&manifest) {
            if let Some(dest) = asset_dest_by_archive_path.get(&archive_path) {
                mapping.insert(
                    original_normalized,
                    dest.finale.to_string_lossy().replace('\\', "/"),
                );
            }
        }
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

        // 4. Application des choix de remappage matériel dans la copie
        //    temporaire, avant la bascule.
        let mut sources_remappees = 0;
        if !choix_valides.is_empty() {
            report("remap", "Remplacement des périphériques…".into(), 0, 1);
            sources_remappees = remap::appliquer(&scenes_dir, &choix_valides)?;
        }
        Ok((assets_restored, sources_remappees))
    })();
    let (assets_restored, sources_remappees) = match preparation {
        Ok(compteurs) => compteurs,
        Err(e) => {
            nettoyer_temporaires();
            return Err(e);
        }
    };

    // 5. Mise en place des assets, avant la bascule : en cas d'échec, les
    //    assets déjà déplacés sont retirés et la configuration active n'a
    //    pas bougé.
    let mut assets_installes: Vec<PathBuf> = Vec::new();
    if tmp_assets.is_some() {
        report("assets", "Mise en place des assets…".into(), 0, 1);
        if let Err(e) = installer_assets(&asset_dest_by_archive_path, &mut assets_installes) {
            retirer_assets(&assets_installes);
            nettoyer_temporaires();
            return Err(e);
        }
    }
    if let Some(transit) = &tmp_assets {
        // Le transit ne contient plus que des dossiers vides.
        let _ = std::fs::remove_dir_all(transit);
    }

    // 6. Bascule avec rollback : l'ancienne config devient une copie de
    //    sécurité, la nouvelle prend sa place ; en cas d'échec, l'ancienne
    //    configuration est remise en place automatiquement et les assets
    //    installés à l'étape 5 sont retirés.
    report("swap", "Mise en place de la configuration…".into(), 0, 1);
    let bak = parent.join(format!("obs-studio.bak-{stamp}"));
    let previous_backup = basculer_config(&target_config, &tmp_config, &bak)
        .inspect_err(|_| retirer_assets(&assets_installes))?;

    // 7. Plugins : jamais installés automatiquement (voir la politique
    //    d'extraction ci-dessus), seulement listés pour réinstallation
    //    manuelle.
    let plugins_status = if manifest.plugins.is_empty() {
        "none".to_string()
    } else {
        "manual".to_string()
    };

    Ok(RestoreSummary {
        scene_collections: manifest.scene_collections.len(),
        profiles: manifest.profiles.len(),
        assets_restored,
        assets_dir: (assets_restored > 0)
            .then(|| assets_dir.unwrap().to_string_lossy().into_owned()),
        plugins_status,
        plugins: manifest.plugins.iter().map(|p| p.name.clone()).collect(),
        previous_config_backup: previous_backup,
        sources_remappees,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        basculer_config, chemin_relatif_sur, installer_assets, retirer_assets, DestinationAsset,
    };
    use std::collections::BTreeMap;
    use std::path::Path;

    /// Prépare un tempdir avec un dossier tmp contenant un fichier marqueur.
    fn bac_a_sable() -> (
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("obs-studio");
        let tmp = dir.path().join("obs-studio.tmp-test");
        let bak = dir.path().join("obs-studio.bak-test");
        std::fs::create_dir(&tmp).unwrap();
        std::fs::write(tmp.join("nouvelle.txt"), "nouvelle config").unwrap();
        (dir, target, tmp, bak)
    }

    #[test]
    fn bascule_nominale_ancienne_config_mise_de_cote() {
        let (_dir, target, tmp, bak) = bac_a_sable();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("ancienne.txt"), "ancienne config").unwrap();

        let previous = basculer_config(&target, &tmp, &bak).unwrap();

        assert_eq!(previous, Some(bak.to_string_lossy().into_owned()));
        assert!(target.join("nouvelle.txt").is_file());
        assert!(bak.join("ancienne.txt").is_file());
        assert!(!tmp.exists());
    }

    #[test]
    fn bascule_premiere_restauration_sans_bak() {
        let (_dir, target, tmp, bak) = bac_a_sable();

        let previous = basculer_config(&target, &tmp, &bak).unwrap();

        assert_eq!(previous, None);
        assert!(target.join("nouvelle.txt").is_file());
        assert!(!bak.exists());
    }

    #[test]
    fn echec_de_mise_de_cote_conserve_la_config_active() {
        let (_dir, target, tmp, bak) = bac_a_sable();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("ancienne.txt"), "ancienne config").unwrap();

        let e = super::basculer_config_avec(&target, &tmp, &bak, |_from, _to| {
            Err(std::io::Error::other("sabotage : verrou simulé"))
        })
        .expect_err("la mise de côté aurait dû échouer");

        assert!(e.contains("mettre de côté"), "{e}");
        assert_eq!(
            std::fs::read_to_string(target.join("ancienne.txt")).unwrap(),
            "ancienne config"
        );
        assert!(!bak.exists());
        assert!(!tmp.exists(), "le dossier temporaire devait être nettoyé");
    }

    #[test]
    fn echec_de_mise_en_place_rollback_automatique() {
        let (_dir, target, tmp, bak) = bac_a_sable();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("ancienne.txt"), "ancienne config").unwrap();

        // Échec déterministe du second rename (tmp → target) via injection.
        let e = super::basculer_config_avec(&target, &tmp, &bak, |from, to| {
            if from == tmp {
                Err(std::io::Error::other("sabotage : verrou simulé"))
            } else {
                std::fs::rename(from, to)
            }
        })
        .expect_err("la bascule aurait dû échouer");

        assert!(e.contains("remise en place"), "{e}");
        // La config d'origine est de nouveau à sa place, contenu intact.
        assert_eq!(
            std::fs::read_to_string(target.join("ancienne.txt")).unwrap(),
            "ancienne config"
        );
        assert!(!bak.exists());
        // Le dossier temporaire orphelin a été nettoyé (best effort).
        assert!(!tmp.exists());
    }

    #[test]
    fn double_echec_le_message_donne_les_chemins_de_reparation() {
        let (_dir, target, tmp, bak) = bac_a_sable();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("ancienne.txt"), "ancienne config").unwrap();

        // Seul le rename n°1 (mise de côté) réussit ; tout le reste échoue,
        // y compris le rollback.
        let e = super::basculer_config_avec(&target, &tmp, &bak, |from, to| {
            if from == target {
                std::fs::rename(from, to)
            } else {
                Err(std::io::Error::other("sabotage total"))
            }
        })
        .expect_err("la bascule aurait dû échouer");

        // Le message doit donner les chemins absolus pour réparer à la main.
        assert!(e.contains(&bak.display().to_string()), "{e}");
        assert!(e.contains(&target.display().to_string()), "{e}");
        assert!(e.contains(&tmp.display().to_string()), "{e}");
        // La config d'origine survit dans le .bak, le tmp n'est pas supprimé.
        assert!(bak.join("ancienne.txt").is_file());
        assert!(tmp.join("nouvelle.txt").is_file());
    }

    #[test]
    fn installation_des_assets_deplace_et_ne_liste_que_les_nouveaux() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transit = dir.path().join("OBS-Backup-Assets.tmp-test");
        let finale = dir.path().join("OBS-Backup-Assets");
        std::fs::create_dir_all(transit.join("0")).unwrap();
        std::fs::create_dir_all(transit.join("1")).unwrap();
        std::fs::write(transit.join("0").join("overlay.png"), "nouveau 0").unwrap();
        std::fs::write(transit.join("1").join("alerte.mp3"), "nouveau 1").unwrap();
        // Collision : un asset au même index/nom existe déjà (restauration
        // précédente) — il est écrasé et ne compte pas comme « nouveau ».
        std::fs::create_dir_all(finale.join("0")).unwrap();
        std::fs::write(finale.join("0").join("overlay.png"), "ancien").unwrap();

        let mut destinations = BTreeMap::new();
        for rel in ["0/overlay.png", "1/alerte.mp3"] {
            destinations.insert(
                format!("assets/{rel}"),
                DestinationAsset {
                    transit: chemin_relatif_sur(&transit, rel).unwrap(),
                    finale: chemin_relatif_sur(&finale, rel).unwrap(),
                },
            );
        }
        let mut installes = Vec::new();
        installer_assets(&destinations, &mut installes).unwrap();

        assert_eq!(
            std::fs::read_to_string(finale.join("0").join("overlay.png")).unwrap(),
            "nouveau 0"
        );
        assert_eq!(
            std::fs::read_to_string(finale.join("1").join("alerte.mp3")).unwrap(),
            "nouveau 1"
        );
        assert_eq!(installes, vec![finale.join("1").join("alerte.mp3")]);
        assert!(!transit.join("0").join("overlay.png").exists());
    }

    #[test]
    fn retrait_des_assets_supprime_fichiers_et_dossiers_vides() {
        let dir = tempfile::tempdir().expect("tempdir");
        let finale = dir.path().join("OBS-Backup-Assets");
        std::fs::create_dir_all(finale.join("0")).unwrap();
        std::fs::create_dir_all(finale.join("1")).unwrap();
        std::fs::write(finale.join("0").join("overlay.png"), "installé").unwrap();
        std::fs::write(finale.join("1").join("alerte.mp3"), "installé").unwrap();
        // Un asset d'une restauration précédente cohabite dans le dossier 1 :
        // le fichier installé est retiré mais le dossier non vide reste.
        std::fs::write(finale.join("1").join("autre.png"), "préexistant").unwrap();

        retirer_assets(&[
            finale.join("0").join("overlay.png"),
            finale.join("1").join("alerte.mp3"),
        ]);

        assert!(!finale.join("0").exists(), "dossier vidé donc supprimé");
        assert!(!finale.join("1").join("alerte.mp3").exists());
        assert!(finale.join("1").join("autre.png").is_file());
    }

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
