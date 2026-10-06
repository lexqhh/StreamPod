//! Pipeline de restauration : fichier .obsbackup → configuration OBS.

use crate::backup::{
    asset_mapping_from_manifest, Manifest, Progress, FORMAT_VERSION, MSG_ANNULATION,
};
use crate::{devices, obs, polices, remap, sanitize, scenes};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

fn err(context: &str, e: impl std::fmt::Display) -> String {
    format!("{context} : {e}")
}

fn chemin_suspect(rel: &str) -> String {
    format!("Archive invalide ou malveillante : chemin suspect \"{rel}\".")
}

/// Nom de périphérique réservé par Windows (`CON`, `NUL`, `COM1`…), avec ou
/// sans extension : `NUL.txt` désigne encore le périphérique.
fn nom_reserve_windows(comp: &str) -> bool {
    const RESERVES: &[&str] = &["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];
    let base = comp
        .split('.')
        .next()
        .unwrap_or(comp)
        .trim_end()
        .to_ascii_uppercase();
    RESERVES.contains(&base.as_str())
        || ((base.starts_with("COM") || base.starts_with("LPT"))
            && base.len() == 4
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
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
            // Windows retire le point ou l'espace final à la création :
            // `obs-browser.` atterrirait dans `obs-browser`, exclu.
            || comp.ends_with(['.', ' '])
            || nom_reserve_windows(comp)
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
    pub services: Vec<ServiceProfil>,
    /// Polices utilisées par les scènes mais absentes de ce PC.
    pub missing_fonts: Vec<String>,
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
    /// Scripts retirés des collections, à réactiver à la main dans OBS
    /// (Outils → Scripts) : chemins après restauration.
    pub scripts: Vec<String>,
}

pub(crate) fn open_archive(backup_path: &Path) -> Result<ZipArchive<File>, String> {
    let file = File::open(backup_path)
        .map_err(|e| err(&format!("Ouverture de {}", backup_path.display()), e))?;
    ZipArchive::new(file)
        .map_err(|e| err("Ce fichier n'est pas une sauvegarde .obsbackup valide", e))
}

/// Plafonds de décompression : une archive très compressible (bombe ZIP) ne
/// doit saturer ni la mémoire à l'aperçu, ni le disque à l'extraction.
const TAILLE_MAX_MANIFEST: u64 = 16 * 1024 * 1024;
const TAILLE_MAX_SERVICE: u64 = 1024 * 1024;
const TAILLE_MAX_ENTREE_CONFIG: u64 = 256 * 1024 * 1024;
const TAILLE_MAX_TOTAL_CONFIG: u64 = 1024 * 1024 * 1024;

fn trop_volumineux(nom: &str) -> String {
    format!("Archive invalide ou malveillante : « {nom} » dépasse la taille autorisée.")
}

/// Lit une entrée texte sans jamais décompresser plus de `limite` octets.
fn lire_texte_borne(entry: impl Read, limite: u64, nom: &str) -> Result<String, String> {
    let mut text = String::new();
    entry
        .take(limite + 1)
        .read_to_string(&mut text)
        .map_err(|e| err(&format!("Lecture de {nom}"), e))?;
    if text.len() as u64 > limite {
        return Err(trop_volumineux(nom));
    }
    Ok(text)
}

fn read_manifest(archive: &mut ZipArchive<File>) -> Result<Manifest, String> {
    let entry = archive
        .by_name("manifest.json")
        .map_err(|_| "Sauvegarde invalide : manifest.json manquant.".to_string())?;
    let text = lire_texte_borne(entry, TAILLE_MAX_MANIFEST, "manifest.json")?;
    let valeur: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| err("Manifest illisible", e))?;
    // Vérifié avant la désérialisation complète : un format futur peut avoir
    // changé de structure, l'utilisateur doit lire « mettez à jour » et non
    // « manifest illisible ».
    let format = valeur
        .get("format_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if format > u64::from(FORMAT_VERSION) {
        return Err(
            "Cette sauvegarde a été créée par une version plus récente de StreamPod. \
             Mettez StreamPod à jour pour la restaurer."
                .to_string(),
        );
    }
    serde_json::from_value(valeur).map_err(|e| err("Manifest illisible", e))
}

/// Service de diffusion d'un profil (`basic/profiles/<profil>/service.json`).
#[derive(Debug, Clone, Serialize)]
pub struct ServiceProfil {
    pub profil: String,
    /// `rtmp_common` (service connu) ou `rtmp_custom` (serveur personnalisé)…
    pub type_service: String,
    pub service: Option<String>,
    pub serveur: Option<String>,
}

/// Lit le service et le serveur de chaque profil de l'archive : une archive
/// fournie par un tiers peut diriger le flux (et la clé re-saisie) vers son
/// propre serveur.
fn lire_services(archive: &mut ZipArchive<File>) -> Result<Vec<ServiceProfil>, String> {
    let mut services = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| err("Lecture de l'archive", e))?;
        let nom = entry.name().to_string();
        let Some(profil) = nom
            .strip_prefix("config/basic/profiles/")
            .and_then(|r| r.split_once('/'))
            .filter(|(_, f)| f.eq_ignore_ascii_case("service.json"))
            .map(|(p, _)| p.to_string())
        else {
            continue;
        };
        let text = lire_texte_borne(entry, TAILLE_MAX_SERVICE, &nom)?;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let champ = |c: &str| {
            v.pointer(&format!("/settings/{c}"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };
        services.push(ServiceProfil {
            profil,
            type_service: v
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            service: champ("service"),
            serveur: champ("server"),
        });
    }
    Ok(services)
}

/// Lit une sauvegarde et prépare le résumé avant restauration.
pub fn preview(backup_path: &Path) -> Result<RestorePreview, String> {
    let mut archive = open_archive(backup_path)?;
    let manifest = read_manifest(&mut archive)?;
    let services = lire_services(&mut archive)?;

    let install_dir = obs::install_dir();
    let installed_version = obs::installed_version();
    let obs_installed = install_dir.is_some();

    let config_exists = obs::config_dir().is_some();

    let mut warnings = Vec::new();
    if obs::version_plus_recente(
        manifest.obs_version.as_deref(),
        installed_version.as_deref(),
    ) {
        warnings.push(format!(
            "Cette sauvegarde vient d'OBS {}, plus récent que la version installée ici ({}). \
             Certains réglages pourraient être ignorés : mettez OBS à jour avant de restaurer.",
            manifest.obs_version.as_deref().unwrap_or_default(),
            installed_version.as_deref().unwrap_or_default()
        ));
    }
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
    for s in services.iter().filter(|s| s.type_service == "rtmp_custom") {
        warnings.push(format!(
            "Le profil « {} » diffuse vers un serveur personnalisé ({}). Vérifiez qu'il \
             s'agit bien du vôtre avant de saisir votre clé de stream.",
            s.profil,
            s.serveur.as_deref().unwrap_or("non renseigné")
        ));
    }
    let missing_fonts = polices::polices_absentes(&manifest.fonts);
    if !missing_fonts.is_empty() {
        warnings.push(format!(
            "Polices à installer : {}. Sans elles, vos textes s'afficheront avec une \
             police de remplacement.",
            missing_fonts.join(" · ")
        ));
    }
    warnings.push(
        "Ne restaurez que vos propres sauvegardes ou celles de personnes de confiance : \
         leurs réglages s'appliqueront à votre OBS."
            .to_string(),
    );

    Ok(RestorePreview {
        manifest,
        backup_file_size: backup_path.metadata().map(|m| m.len()).unwrap_or(0),
        obs_installed,
        installed_version,
        config_exists,
        services,
        missing_fonts,
        warnings,
    })
}

/// Extrait une entrée du ZIP vers un fichier sur le disque, par blocs de
/// 512 Ko. Les plafonds portent sur les octets réellement écrits (la taille
/// déclarée dans l'archive n'est pas fiable) : `plafond` pour l'entrée,
/// `budget` pour le cumul, décrémenté au fil de l'eau.
fn extract_entry(
    archive: &mut ZipArchive<File>,
    index: usize,
    dest: &Path,
    plafond: u64,
    budget: &mut u64,
    est_annule: &dyn Fn() -> bool,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| err(&format!("Création du dossier {}", parent.display()), e))?;
    }
    let mut entry = archive
        .by_index(index)
        .map_err(|e| err("Lecture de l'archive", e))?;
    let mut out =
        File::create(dest).map_err(|e| err(&format!("Création de {}", dest.display()), e))?;
    let extraction = |e| err(&format!("Extraction vers {}", dest.display()), e);
    let mut buf = vec![0u8; 512 * 1024];
    let mut ecrits = 0u64;
    loop {
        if est_annule() {
            return Err(MSG_ANNULATION.to_string());
        }
        let n = entry.read(&mut buf).map_err(extraction)?;
        if n == 0 {
            return Ok(());
        }
        ecrits += n as u64;
        if ecrits > plafond || n as u64 > *budget {
            return Err(trop_volumineux(entry.name()));
        }
        *budget -= n as u64;
        out.write_all(&buf[..n]).map_err(extraction)?;
    }
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
    basculer_config_avec(target, tmp, bak, true, |from, to| std::fs::rename(from, to))
}

/// Retour à une copie de sécurité (`copie` prend la place de `target`, mise
/// de côté dans `bak`) : même garantie de rollback, mais la copie n'est
/// jamais supprimée en cas d'échec.
pub(crate) fn basculer_vers_copie(
    target: &Path,
    copie: &Path,
    bak: &Path,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<Option<String>, String> {
    basculer_config_avec(target, copie, bak, false, rename)
}

/// Variante à fonction de rename injectable, pour tester les scénarios
/// d'échec de façon déterministe. `nettoyer_tmp` : supprimer `tmp` si la
/// bascule échoue (extraction temporaire), jamais pour une copie de sécurité.
fn basculer_config_avec(
    target: &Path,
    tmp: &Path,
    bak: &Path,
    nettoyer_tmp: bool,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<Option<String>, String> {
    let mut previous_backup = None;
    if target.exists() {
        if let Err(e) = rename(target, bak) {
            // Rien n'a encore bougé : la configuration active est intacte et
            // l'extraction temporaire peut être supprimée sans risque.
            if nettoyer_tmp {
                let _ = std::fs::remove_dir_all(tmp);
            }
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
            if nettoyer_tmp {
                let _ = std::fs::remove_dir_all(tmp);
            }
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
        if nettoyer_tmp {
            let _ = std::fs::remove_dir_all(tmp);
        }
        return Err(format!(
            "La restauration a échoué ({e}). Votre configuration d'origine a été remise en \
             place : la configuration OBS active n'a pas été modifiée."
        ));
    }
    Ok(previous_backup)
}

/// Destination d'un asset : extrait d'abord dans un dossier de transit propre
/// à cette restauration, puis déplacé vers son emplacement définitif une fois
/// toute la préparation réussie - le dossier d'assets définitif n'est jamais
/// touché par une restauration qui échoue en cours de route.
struct DestinationAsset {
    transit: PathBuf,
    finale: PathBuf,
    /// Taille déclarée au manifest : plafond d'extraction de l'entrée.
    taille: u64,
}

/// Déplace les assets du dossier de transit vers leur emplacement définitif,
/// propre à cette restauration (`OBS-Backup-Assets\<horodatage>\…`) : aucun
/// fichier existant n'est jamais écrasé. Les fichiers déplacés sont ajoutés
/// à `installes` au fil de l'eau, pour pouvoir les retirer si la suite de la
/// restauration échoue.
fn installer_assets(
    destinations: &BTreeMap<String, DestinationAsset>,
    installes: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for dest in destinations.values() {
        // Entrée listée au manifest mais absente de l'archive : rien à faire.
        if !dest.transit.is_file() {
            continue;
        }
        if dest.finale.exists() {
            return Err(format!(
                "Le fichier {} existe déjà : restauration interrompue pour ne rien écraser.",
                dest.finale.display()
            ));
        }
        if let Some(parent) = dest.finale.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| err(&format!("Création du dossier {}", parent.display()), e))?;
        }
        std::fs::rename(&dest.transit, &dest.finale)
            .map_err(|e| err(&format!("Mise en place de {}", dest.finale.display()), e))?;
        installes.push(dest.finale.clone());
    }
    Ok(())
}

/// Retire (best effort) les assets installés par une restauration qui a
/// échoué ensuite, ainsi que leurs dossiers (`<n>` puis `<horodatage>`)
/// devenus vides.
fn retirer_assets(installes: &[PathBuf]) {
    for fichier in installes {
        let _ = std::fs::remove_file(fichier);
        // remove_dir échoue sur un dossier non vide : rien d'autre n'est touché.
        for dossier in fichier.ancestors().skip(1).take(2) {
            let _ = std::fs::remove_dir(dossier);
        }
    }
}

/// Applique la neutralisation d'obs-websocket à une config extraite, quoi que
/// dise l'archive (ancienne ou piégée) ; un JSON illisible est retiré, OBS
/// recrée alors ses réglages par défaut.
fn neutraliser_obs_websocket_extrait(config_dir: &Path) -> Result<(), String> {
    let ws = config_dir
        .join("plugin_config")
        .join("obs-websocket")
        .join("config.json");
    if !ws.is_file() {
        return Ok(());
    }
    match std::fs::read_to_string(&ws)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    {
        Some(mut v) if v.is_object() => {
            sanitize::neutraliser_obs_websocket(&mut v);
            std::fs::write(&ws, serde_json::to_string_pretty(&v).unwrap())
                .map_err(|e| err(&format!("Écriture de {}", ws.display()), e))
        }
        _ => std::fs::remove_file(&ws)
            .map_err(|e| err(&format!("Suppression de {}", ws.display()), e)),
    }
}

/// Restaure une sauvegarde .obsbackup en appliquant les choix de remappage
/// matériel confirmés par l'utilisateur (`choix` peut être vide), sans
/// revérifier OBS : voir `restore_avec_garde`.
pub fn restore(
    backup_path: &Path,
    choix: &[remap::Choix],
    progress: impl Fn(Progress),
    est_annule: impl Fn() -> bool,
) -> Result<RestoreSummary, String> {
    restore_avec_garde(backup_path, choix, progress, est_annule, || false)
}

/// Comme `restore`, mais `obs_ouvert` (injectable en test) est revérifié
/// juste avant la bascule : OBS a pu être lancé pendant l'extraction. Le
/// refus initial si OBS tourne reste à la charge de l'appelant (commande
/// `restore_run` dans lib.rs).
pub fn restore_avec_garde(
    backup_path: &Path,
    choix: &[remap::Choix],
    progress: impl Fn(Progress),
    est_annule: impl Fn() -> bool,
    obs_ouvert: impl Fn() -> bool,
) -> Result<RestoreSummary, String> {
    let report = |step: &str, message: String, current: u64, total: u64| {
        progress(Progress {
            step: step.to_string(),
            message,
            current,
            total,
        });
    };

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
    //    préparation réussie. Chaque restauration a son propre sous-dossier
    //    horodaté : rien d'existant n'est écrasé.
    let racine_assets = obs::assets_target_dir();
    let assets_dir = racine_assets.as_ref().map(|d| d.join(&stamp));
    let mut asset_dest_by_archive_path: BTreeMap<String, DestinationAsset> = BTreeMap::new();
    let mut tmp_assets: Option<PathBuf> = None;
    if !manifest.assets.is_empty() {
        let (Some(racine), Some(assets_dir)) = (&racine_assets, &assets_dir) else {
            return Err("Impossible de déterminer le dossier Documents pour les assets.".into());
        };
        let transit = racine.with_file_name(format!(
            "{}.tmp-{stamp}",
            racine.file_name().unwrap_or_default().to_string_lossy()
        ));
        for asset in &manifest.assets {
            // Exécutable déclaré par le manifest : ni extrait ni référencé.
            if scenes::est_executable(&asset.archive_path)
                || scenes::est_executable(&asset.original_path)
            {
                continue;
            }
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
                    taille: asset.size,
                },
            );
        }
        tmp_assets = Some(transit);
    }
    // Dossiers de diaporama ou de playlist : le chemin du dossier est
    // réécrit vers son emplacement restauré (validé comme les assets).
    let mut dossiers_dest: Vec<(String, PathBuf)> = Vec::new();
    if let Some(assets_dir) = assets_dir.as_ref().filter(|_| tmp_assets.is_some()) {
        for d in &manifest.asset_dirs {
            let rel = d
                .archive_dir
                .strip_prefix("assets/")
                .ok_or_else(|| chemin_suspect(&d.archive_dir))?;
            dossiers_dest.push((
                scenes::normalize_path(&d.original_path),
                chemin_relatif_sur(assets_dir, rel)?,
            ));
        }
    }

    // Toute erreur entre l'extraction et la bascule supprime les deux
    // dossiers temporaires : ni configuration partielle, ni assets de
    // transit orphelins - la configuration active et le dossier d'assets
    // définitif n'ont pas bougé.
    let nettoyer_temporaires = || {
        let _ = std::fs::remove_dir_all(&tmp_config);
        if let Some(transit) = &tmp_assets {
            let _ = std::fs::remove_dir_all(transit);
        }
    };

    let total_entries = archive.len() as u64;

    // Politique d'extraction : une entrée hors des préfixes connus (config/,
    // assets/ du manifest) est ignorée sans erreur - compatibilité
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
    let preparation = (|| -> Result<(usize, usize, Vec<String>), String> {
        let mut assets_restored = 0usize;
        // Budgets de décompression : la config est bornée forfaitairement,
        // les assets par les tailles déclarées au manifest.
        let mut budget_config = TAILLE_MAX_TOTAL_CONFIG;
        let mut budget_assets: u64 = asset_dest_by_archive_path.values().map(|d| d.taille).sum();
        for i in 0..archive.len() {
            if est_annule() {
                return Err(MSG_ANNULATION.to_string());
            }
            let (name, is_dir) = {
                let entry = archive
                    .by_index(i)
                    .map_err(|e| err("Lecture de l'archive", e))?;
                (entry.name().to_string(), entry.is_dir())
            };
            if is_dir {
                continue;
            }
            report(
                "extract",
                format!("Extraction : {name}"),
                i as u64,
                total_entries,
            );

            if let Some(rel) = name.strip_prefix("config/") {
                // Entrée exclue à la sauvegarde mais présente dans une archive
                // plus ancienne (.sentinel/ avant la v0.1.2) : ignorée.
                if sanitize::is_excluded_config_path(&rel.to_lowercase()) {
                    continue;
                }
                let dest = chemin_relatif_sur(&tmp_config, rel)?;
                extract_entry(
                    &mut archive,
                    i,
                    &dest,
                    TAILLE_MAX_ENTREE_CONFIG,
                    &mut budget_config,
                    &est_annule,
                )?;
            } else if name.starts_with("assets/") {
                if let Some(dest) = asset_dest_by_archive_path.get(&name) {
                    extract_entry(
                        &mut archive,
                        i,
                        &dest.transit,
                        dest.taille,
                        &mut budget_assets,
                        &est_annule,
                    )?;
                    assets_restored += 1;
                }
            }
        }

        neutraliser_obs_websocket_extrait(&tmp_config)?;

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
        for (original, dossier) in &dossiers_dest {
            mapping.insert(
                original.clone(),
                dossier.to_string_lossy().replace('\\', "/"),
            );
        }
        // Les scripts (modules["scripts-tool"]) sont neutralisés dans toutes
        // les collections : OBS les exécuterait au lancement. Leurs fichiers
        // restent restaurés avec les assets, pour réactivation manuelle.
        let mut scripts: Vec<String> = Vec::new();
        if scenes_dir.is_dir() {
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
                let reecrits = scenes::rewrite_asset_paths(&mut value, &mapping);
                let retires = scenes::retirer_scripts(&mut value);
                if reecrits == 0 && retires.is_empty() {
                    continue;
                }
                scripts.extend(retires);
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
        Ok((assets_restored, sources_remappees, scripts))
    })();
    let (assets_restored, sources_remappees, scripts) = match preparation {
        Ok(compteurs) => compteurs,
        Err(e) => {
            nettoyer_temporaires();
            return Err(e);
        }
    };

    // Dernier point d'annulation : au-delà, la restauration modifie le dossier
    // d'assets définitif puis bascule la configuration - on va jusqu'au bout
    // (garantie « jamais d'état sans config »), l'annulation est ignorée.
    if est_annule() {
        nettoyer_temporaires();
        return Err(MSG_ANNULATION.to_string());
    }

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

    // OBS lancé pendant l'extraction : il réécrirait sa config par-dessus la
    // restauration à sa fermeture. Rien n'a encore basculé.
    if obs_ouvert() {
        retirer_assets(&assets_installes);
        nettoyer_temporaires();
        return Err(obs::OBS_RUNNING_MSG.to_string());
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
        scripts,
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

        let e = super::basculer_config_avec(&target, &tmp, &bak, true, |_from, _to| {
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
        let e = super::basculer_config_avec(&target, &tmp, &bak, true, |from, to| {
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
        let e = super::basculer_config_avec(&target, &tmp, &bak, true, |from, to| {
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
    fn installation_des_assets_deplace_sans_jamais_ecraser() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transit = dir.path().join("OBS-Backup-Assets.tmp-test");
        let finale = dir.path().join("OBS-Backup-Assets").join("20260101-120000");
        std::fs::create_dir_all(transit.join("0")).unwrap();
        std::fs::create_dir_all(transit.join("1")).unwrap();
        std::fs::write(transit.join("0").join("overlay.png"), "nouveau 0").unwrap();
        std::fs::write(transit.join("1").join("alerte.mp3"), "nouveau 1").unwrap();

        let destinations = |rels: &[&str]| -> BTreeMap<String, DestinationAsset> {
            rels.iter()
                .map(|rel| {
                    (
                        format!("assets/{rel}"),
                        DestinationAsset {
                            transit: chemin_relatif_sur(&transit, rel).unwrap(),
                            finale: chemin_relatif_sur(&finale, rel).unwrap(),
                            taille: 9,
                        },
                    )
                })
                .collect()
        };
        let mut installes = Vec::new();
        installer_assets(
            &destinations(&["0/overlay.png", "1/alerte.mp3"]),
            &mut installes,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(finale.join("0").join("overlay.png")).unwrap(),
            "nouveau 0"
        );
        assert_eq!(
            installes,
            vec![
                finale.join("0").join("overlay.png"),
                finale.join("1").join("alerte.mp3")
            ]
        );
        assert!(!transit.join("0").join("overlay.png").exists());

        // Collision (même horodatage) : refus, le fichier en place est intact.
        std::fs::write(transit.join("0").join("overlay.png"), "intrus").unwrap();
        let mut installes = Vec::new();
        let e = installer_assets(&destinations(&["0/overlay.png"]), &mut installes)
            .expect_err("un asset existant ne doit jamais être écrasé");
        assert!(e.contains("existe déjà"), "{e}");
        assert!(installes.is_empty());
        assert_eq!(
            std::fs::read_to_string(finale.join("0").join("overlay.png")).unwrap(),
            "nouveau 0"
        );
    }

    #[test]
    fn retrait_des_assets_supprime_fichiers_et_dossiers_vides() {
        let dir = tempfile::tempdir().expect("tempdir");
        let racine = dir.path().join("OBS-Backup-Assets");
        let finale = racine.join("20260101-120000");
        std::fs::create_dir_all(finale.join("0")).unwrap();
        std::fs::create_dir_all(finale.join("1")).unwrap();
        std::fs::write(finale.join("0").join("overlay.png"), "installé").unwrap();
        std::fs::write(finale.join("1").join("alerte.mp3"), "installé").unwrap();
        // Une restauration précédente cohabite dans la racine : intacte.
        std::fs::create_dir_all(racine.join("20251231-090000")).unwrap();

        retirer_assets(&[
            finale.join("0").join("overlay.png"),
            finale.join("1").join("alerte.mp3"),
        ]);

        assert!(!finale.exists(), "dossier horodaté vidé donc supprimé");
        assert!(racine.join("20251231-090000").is_dir());
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
        // Proches des noms réservés, mais légitimes.
        for rel in [
            "console.json",
            "COM10.png",
            "nullable.ini",
            "Auxiliaire.json",
            ".sentinel",
        ] {
            assert!(chemin_relatif_sur(base, rel).is_ok(), "{rel}");
        }
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
            "plugin_config/obs-browser./Cookies",
            ".sentinel./x",
            "a/dossier /x",
            "a/fichier.",
            "CON",
            "a/nul.txt",
            "Com1.json",
            "lpt9",
            "aux .ini",
        ] {
            let e = chemin_relatif_sur(base, rel)
                .expect_err(&format!("« {rel} » aurait dû être rejeté"));
            assert!(e.contains("Archive invalide ou malveillante"), "{e}");
        }
    }
}
