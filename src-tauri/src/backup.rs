//! Pipeline de sauvegarde : dossier de config OBS → fichier .obsbackup (ZIP).

use crate::sanitize;
use crate::scenes;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// Version du format de fichier .obsbackup.
pub const FORMAT_VERSION: u32 = 1;

/// Message renvoyé quand l'utilisateur annule une opération en cours.
/// L'UI compare ce texte pour distinguer une annulation d'une vraie erreur.
pub const MSG_ANNULATION: &str = "Opération annulée.";

/// DLL livrées avec OBS Studio (ou runtime CEF) : tout autre .dll dans
/// obs-plugins\64bit est considéré comme un plugin tiers.
const OFFICIAL_PLUGIN_STEMS: &[&str] = &[
    "aja",
    "aja-output-ui",
    "coreaudio-encoder",
    "decklink",
    "decklink-captions",
    "decklink-output-ui",
    "frontend-tools",
    "image-source",
    "obs-browser",
    "obs-browser-page",
    "obs-ffmpeg",
    "obs-filters",
    "obs-nvenc",
    "nv-filters",
    "obs-outputs",
    "obs-qsv11",
    "obs-text",
    "obs-transitions",
    "obs-vst",
    "obs-websocket",
    "obs-webrtc",
    "obs-x264",
    "rtmp-services",
    "text-freetype2",
    "vlc-video",
    "win-capture",
    "win-dshow",
    "win-wasapi",
    // Runtime CEF / dépendances embarquées avec OBS.
    "libcef",
    "chrome_elf",
    "libegl",
    "libglesv2",
    "vk_swiftshader",
    "vulkan-1",
    "d3dcompiler_47",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub dll: String,
    pub size: u64,
    pub has_data_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetEntry {
    /// Chemin absolu d'origine sur la machine sauvegardée.
    pub original_path: String,
    /// Chemin dans l'archive (assets/<n>/<nom de fichier>).
    pub archive_path: String,
    pub file_name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub app_version: String,
    pub created_at: String,
    pub obs_version: Option<String>,
    pub scene_collections: Vec<String>,
    pub profiles: Vec<String>,
    pub plugins: Vec<PluginInfo>,
    pub assets: Vec<AssetEntry>,
}

/// Résumé présenté à l'utilisateur avant de lancer la sauvegarde.
#[derive(Debug, Clone, Serialize)]
pub struct BackupPreview {
    pub config_dir: String,
    pub obs_version: Option<String>,
    pub scene_collections: Vec<String>,
    pub profiles: Vec<String>,
    pub plugins: Vec<PluginInfo>,
    pub asset_count: usize,
    pub asset_total_size: u64,
    pub missing_assets: Vec<String>,
    pub browser_sources: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupSummary {
    pub output_path: String,
    pub file_size: u64,
    pub scene_collections: usize,
    pub profiles: usize,
    pub plugins: usize,
    pub assets: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub step: String,
    pub message: String,
    pub current: u64,
    pub total: u64,
}

fn err(context: &str, e: impl std::fmt::Display) -> String {
    format!("{context} : {e}")
}

/// Liste les fichiers de collections de scènes (basic/scenes/*.json).
fn scene_files(config_dir: &Path) -> Vec<PathBuf> {
    let dir = config_dir.join("basic").join("scenes");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))
                && !p
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().ends_with(".json.bak"))
        })
        .collect();
    files.sort();
    files
}

/// Liste les profils (sous-dossiers de basic/profiles).
fn profile_dirs(config_dir: &Path) -> Vec<PathBuf> {
    let dir = config_dir.join("basic").join("profiles");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Collecte tous les chemins d'assets référencés par toutes les collections
/// de scènes. Retourne aussi les chemins référencés mais introuvables.
fn collect_assets(config_dir: &Path) -> (BTreeSet<PathBuf>, Vec<String>) {
    let mut found = BTreeSet::new();
    let mut missing = BTreeSet::new();
    for scene_file in scene_files(config_dir) {
        let Ok(text) = std::fs::read_to_string(&scene_file) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let mut all_refs = BTreeSet::new();
        collect_path_like_strings(&value, &mut all_refs);
        for r in all_refs {
            let p = PathBuf::from(&r);
            if p.is_file() {
                found.insert(p);
            } else if !p.is_dir() {
                missing.insert(r);
            }
        }
    }
    (found, missing.into_iter().collect())
}

/// Comme scenes::collect_asset_paths mais garde aussi les chemins absents
/// du disque (pour prévenir l'utilisateur des assets introuvables).
fn collect_path_like_strings(value: &serde_json::Value, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::String(s) => {
            let b = s.as_bytes();
            if s.len() >= 4
                && b[0].is_ascii_alphabetic()
                && b[1] == b':'
                && (b[2] == b'\\' || b[2] == b'/')
                && !s.contains('\n')
            {
                out.insert(s.clone());
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_path_like_strings(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values() {
                collect_path_like_strings(v, out);
            }
        }
        _ => {}
    }
}

/// Compte les sources navigateur (overlays) dont l'URL pointe vers le web.
/// Ces URL sont conservées telles quelles dans l'archive (OBS en a besoin pour
/// réafficher l'overlay) et peuvent contenir un token privé
/// (`…/overlay/<id>/<TOKEN>` chez StreamElements, Streamlabs…) : l'aperçu
/// doit prévenir l'utilisateur avant qu'il ne partage le fichier.
fn count_browser_sources(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Array(items) => items.iter().map(count_browser_sources).sum(),
        serde_json::Value::Object(map) => {
            let ici = usize::from(
                map.get("id").and_then(|v| v.as_str()) == Some("browser_source")
                    && map
                        .get("settings")
                        .and_then(|s| s.get("url"))
                        .and_then(|u| u.as_str())
                        .is_some_and(|u| {
                            let u = u.to_ascii_lowercase();
                            u.starts_with("http://") || u.starts_with("https://")
                        }),
            );
            ici + map.values().map(count_browser_sources).sum::<usize>()
        }
        _ => 0,
    }
}

/// Détecte les plugins tiers dans le dossier d'installation d'OBS.
fn third_party_plugins(install_dir: &Path) -> Vec<PluginInfo> {
    let plugin_dir = install_dir.join("obs-plugins").join("64bit");
    let mut plugins: Vec<PluginInfo> = std::fs::read_dir(&plugin_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll"))
        })
        .filter_map(|p| {
            let stem = p.file_stem()?.to_string_lossy().to_lowercase();
            if OFFICIAL_PLUGIN_STEMS.contains(&stem.as_str()) {
                return None;
            }
            let size = p.metadata().map(|m| m.len()).unwrap_or(0);
            let data_dir = install_dir.join("data").join("obs-plugins").join(&stem);
            Some(PluginInfo {
                name: p.file_stem().unwrap().to_string_lossy().into_owned(),
                dll: p.file_name().unwrap().to_string_lossy().into_owned(),
                size,
                has_data_dir: data_dir.is_dir(),
            })
        })
        .collect();
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    plugins
}

/// Prépare le résumé avant sauvegarde.
pub fn preview(
    config_dir: &Path,
    install_dir: Option<&Path>,
    obs_version: Option<String>,
) -> Result<BackupPreview, String> {
    if !config_dir.is_dir() {
        return Err("Dossier de configuration OBS introuvable.".to_string());
    }
    let scene_names = scene_files(config_dir)
        .iter()
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    let profile_names = profile_dirs(config_dir)
        .iter()
        .filter_map(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    let (assets, missing) = collect_assets(config_dir);
    let browser_sources = scene_files(config_dir)
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .map(|value| count_browser_sources(&value))
        .sum();
    let asset_total_size = assets
        .iter()
        .map(|p| p.metadata().map(|m| m.len()).unwrap_or(0))
        .sum();
    let plugins = install_dir.map(third_party_plugins).unwrap_or_default();
    Ok(BackupPreview {
        config_dir: config_dir.to_string_lossy().into_owned(),
        obs_version,
        scene_collections: scene_names,
        profiles: profile_names,
        plugins,
        asset_count: assets.len(),
        asset_total_size,
        missing_assets: missing,
        browser_sources,
    })
}

/// Copie un fichier du disque vers l'archive ZIP. `est_annule` est consulté
/// entre chaque bloc de 512 Ko pour réagir vite même sur un gros média.
fn zip_file_from_disk(
    zip: &mut ZipWriter<File>,
    src: &Path,
    archive_path: &str,
    options: SimpleFileOptions,
    est_annule: &dyn Fn() -> bool,
) -> Result<(), String> {
    zip.start_file(archive_path, options)
        .map_err(|e| err("Écriture de l'archive", e))?;
    let mut f = File::open(src).map_err(|e| err(&format!("Lecture de {}", src.display()), e))?;
    let mut buf = [0u8; 1024 * 512];
    loop {
        if est_annule() {
            return Err(MSG_ANNULATION.to_string());
        }
        let n = f
            .read(&mut buf)
            .map_err(|e| err(&format!("Lecture de {}", src.display()), e))?;
        if n == 0 {
            break;
        }
        zip.write_all(&buf[..n])
            .map_err(|e| err("Écriture de l'archive", e))?;
    }
    Ok(())
}

/// Le dossier de destination de l'archive est-il le dossier de config OBS
/// ou l'un de ses sous-dossiers ?
fn destination_dans_config(config_dir: &Path, output_path: &Path) -> bool {
    let parent = match output_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    // Si le dossier de destination n'existe pas encore, File::create échouera
    // de toute façon avec un message clair : pas de refus ici.
    let (Ok(config), Ok(parent)) = (config_dir.canonicalize(), parent.canonicalize()) else {
        return false;
    };
    let config = scenes::normalize_path(&config.to_string_lossy());
    let parent = scenes::normalize_path(&parent.to_string_lossy());
    parent == config || parent.starts_with(&format!("{config}/"))
}

/// Supprime le fichier temporaire si la sauvegarde échoue avant la bascule.
struct NettoyageTmp<'a> {
    path: &'a Path,
    actif: bool,
}

impl Drop for NettoyageTmp<'_> {
    fn drop(&mut self) {
        if self.actif {
            let _ = std::fs::remove_file(self.path);
        }
    }
}

/// Crée le fichier .obsbackup.
pub fn create(
    config_dir: &Path,
    install_dir: Option<&Path>,
    obs_version: Option<String>,
    output_path: &Path,
    progress: impl Fn(Progress),
    est_annule: impl Fn() -> bool,
) -> Result<BackupSummary, String> {
    if !config_dir.is_dir() {
        return Err("Dossier de configuration OBS introuvable.".to_string());
    }

    let report = |step: &str, message: String, current: u64, total: u64| {
        progress(Progress {
            step: step.to_string(),
            message,
            current,
            total,
        });
    };

    report("scan", "Analyse de la configuration…".into(), 0, 1);

    // 1. Assets référencés par les scènes.
    let (asset_paths, _missing) = collect_assets(config_dir);
    let mut assets: Vec<AssetEntry> = Vec::new();
    for (i, p) in asset_paths.iter().enumerate() {
        let file_name = scenes::file_name_of(p);
        assets.push(AssetEntry {
            original_path: p.to_string_lossy().into_owned(),
            archive_path: format!("assets/{i}/{file_name}"),
            file_name,
            size: p.metadata().map(|m| m.len()).unwrap_or(0),
        });
    }

    // 2. Plugins tiers.
    let plugins = install_dir.map(third_party_plugins).unwrap_or_default();

    let scene_names: Vec<String> = scene_files(config_dir)
        .iter()
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    let profile_names: Vec<String> = profile_dirs(config_dir)
        .iter()
        .filter_map(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        .collect();

    // 3. Création de l'archive.
    // Refus d'une destination à l'intérieur du dossier de config : l'archive
    // serait ramassée par le parcours ci-dessous et zip_file_from_disk la
    // lirait pendant qu'elle grossit — chaque lecture provoque une écriture
    // plus loin dans le même fichier, le EOF n'arrive jamais et le disque
    // se remplit. L'exclusion des .bak ne couvre pas ce cas.
    if destination_dans_config(config_dir, output_path) {
        return Err(
            "La sauvegarde ne peut pas être enregistrée dans le dossier de configuration OBS. Choisissez un autre emplacement.".to_string(),
        );
    }
    // Écriture dans un fichier temporaire puis bascule : la destination ne
    // contient jamais d'archive incomplète, même en cas d'échec en cours
    // de route.
    let tmp_path = output_path.with_extension("obsbackup.tmp");
    let file = File::create(&tmp_path)
        .map_err(|e| err(&format!("Création de {}", tmp_path.display()), e))?;
    let mut nettoyage = NettoyageTmp {
        path: &tmp_path,
        actif: true,
    };
    let mut zip = ZipWriter::new(file);
    let deflate = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    // Les médias sont déjà compressés : on les stocke tels quels (rapide),
    // en autorisant les fichiers > 4 Go.
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true);

    // 3a. Configuration (nettoyée des secrets).
    let config_files: Vec<PathBuf> = WalkDir::new(config_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    let total_cfg = config_files.len() as u64;
    for (i, path) in config_files.iter().enumerate() {
        if est_annule() {
            return Err(MSG_ANNULATION.to_string());
        }
        let rel = path
            .strip_prefix(config_dir)
            .map_err(|e| err("Chemin de configuration inattendu", e))?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let rel_lower = rel_str.to_lowercase();
        if sanitize::is_excluded_config_path(&rel_lower) {
            continue;
        }
        // Copies de sécurité internes d'OBS (service.json.bak…) : elles
        // contiennent les mêmes secrets que les originaux, on les exclut.
        if rel_lower.ends_with(".bak") {
            continue;
        }
        report(
            "config",
            format!("Configuration : {rel_str}"),
            i as u64,
            total_cfg,
        );
        let archive_path = format!("config/{rel_str}");
        let is_service_json = rel_str.to_lowercase().ends_with("/service.json");
        let is_ini = rel_str.to_lowercase().ends_with(".ini");
        // Autres JSON de plugins (obs-websocket/config.json…) : peuvent
        // contenir des secrets (mot de passe serveur…) et doivent être
        // nettoyés comme service.json. On exclut volontairement les scènes
        // (basic/scenes/**), qui contiennent des champs `key` légitimes
        // (raccourcis clavier) — celles-ci ne passent jamais par ici car
        // elles ne sont pas sous plugin_config/.
        let is_plugin_json = rel_lower.starts_with("plugin_config/") && rel_lower.ends_with(".json");
        if is_service_json {
            let text = std::fs::read_to_string(path)
                .map_err(|e| err(&format!("Lecture de {rel_str}"), e))?;
            let mut value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| err(&format!("JSON invalide dans {rel_str}"), e))?;
            sanitize::sanitize_json(&mut value);
            zip.start_file(&archive_path, deflate)
                .map_err(|e| err("Écriture de l'archive", e))?;
            zip.write_all(serde_json::to_string_pretty(&value).unwrap().as_bytes())
                .map_err(|e| err("Écriture de l'archive", e))?;
        } else if is_ini {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            let clean = sanitize::sanitize_ini(&text);
            zip.start_file(&archive_path, deflate)
                .map_err(|e| err("Écriture de l'archive", e))?;
            zip.write_all(clean.as_bytes())
                .map_err(|e| err("Écriture de l'archive", e))?;
        } else if is_plugin_json {
            // Un JSON de plugin illisible ou invalide n'est PAS copié tel
            // quel (fuite potentielle) : on l'exclut de l'archive et on le
            // signale, sans faire échouer toute la sauvegarde.
            let parsed = std::fs::read_to_string(path)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
            match parsed {
                Some(mut value) => {
                    let removed = sanitize::sanitize_json(&mut value);
                    if removed > 0 {
                        report(
                            "warning",
                            format!(
                                "{removed} champ(s) sensible(s) retiré(s) de la configuration de plugin : {rel_str}"
                            ),
                            i as u64,
                            total_cfg,
                        );
                    }
                    zip.start_file(&archive_path, deflate)
                        .map_err(|e| err("Écriture de l'archive", e))?;
                    zip.write_all(serde_json::to_string_pretty(&value).unwrap().as_bytes())
                        .map_err(|e| err("Écriture de l'archive", e))?;
                }
                None => {
                    report(
                        "warning",
                        format!(
                            "Configuration de plugin illisible ou invalide, exclue de la sauvegarde par précaution : {rel_str}"
                        ),
                        i as u64,
                        total_cfg,
                    );
                }
            }
        } else if rel_lower.starts_with("plugin_config/") {
            // Liste blanche sous plugin_config/ : seuls les .json et .ini
            // assainis (branches ci-dessus) sont archivés. Tout autre format
            // (tokens.sqlite, credentials.yaml…) est une donnée opaque d'un
            // plugin tiers pouvant contenir des secrets : jamais copié brut.
            report(
                "warning",
                format!(
                    "Fichier de configuration de plugin dans un format non pris en charge, exclu de la sauvegarde par précaution : {rel_str}"
                ),
                i as u64,
                total_cfg,
            );
        } else {
            zip_file_from_disk(&mut zip, path, &archive_path, deflate, &est_annule)?;
        }
    }

    // 3b. Assets.
    let total_assets = assets.len() as u64;
    for (i, asset) in assets.iter().enumerate() {
        if est_annule() {
            return Err(MSG_ANNULATION.to_string());
        }
        report(
            "assets",
            format!("Assets : {}", asset.file_name),
            i as u64,
            total_assets,
        );
        zip_file_from_disk(
            &mut zip,
            Path::new(&asset.original_path),
            &asset.archive_path,
            stored,
            &est_annule,
        )?;
    }

    // 3c. Plugins tiers : jamais archivés. Les DLL ne sont de toute façon
    // jamais restaurées (une archive est une donnée non fiable) — seule la
    // liste du manifest sert, pour la réinstallation manuelle. Les archiver
    // n'apportait que du poids mort de binaires opaques.

    // 3d. Manifest.
    report("finalize", "Finalisation…".into(), 0, 1);
    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: chrono::Local::now().to_rfc3339(),
        obs_version,
        scene_collections: scene_names.clone(),
        profiles: profile_names.clone(),
        plugins: plugins.clone(),
        assets: assets.clone(),
    };
    zip.start_file("manifest.json", deflate)
        .map_err(|e| err("Écriture de l'archive", e))?;
    zip.write_all(serde_json::to_string_pretty(&manifest).unwrap().as_bytes())
        .map_err(|e| err("Écriture de l'archive", e))?;
    zip.finish().map_err(|e| err("Finalisation de l'archive", e))?;
    std::fs::rename(&tmp_path, output_path)
        .map_err(|e| err(&format!("Mise en place de {}", output_path.display()), e))?;
    nettoyage.actif = false;

    let file_size = output_path.metadata().map(|m| m.len()).unwrap_or(0);
    Ok(BackupSummary {
        output_path: output_path.to_string_lossy().into_owned(),
        file_size,
        scene_collections: scene_names.len(),
        profiles: profile_names.len(),
        plugins: plugins.len(),
        assets: assets.len(),
    })
}

/// Table de correspondance chemin d'origine (normalisé) → chemin d'archive,
/// reconstruite depuis le manifest à la restauration.
pub fn asset_mapping_from_manifest(manifest: &Manifest) -> BTreeMap<String, String> {
    manifest
        .assets
        .iter()
        .map(|a| {
            (
                scenes::normalize_path(&a.original_path),
                a.archive_path.clone(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::count_browser_sources;
    use serde_json::json;

    #[test]
    fn les_sources_navigateur_web_sont_comptees_meme_imbriquees() {
        let scene = json!({
            "sources": [
                { "id": "browser_source",
                  "settings": { "url": "https://streamelements.com/overlay/abc/TOKEN" } },
                { "id": "browser_source",
                  "settings": { "url": "HTTP://exemple.test/alertes" } },
                { "id": "group", "settings": { "items": [
                    { "id": "browser_source",
                      "settings": { "url": "https://streamlabs.com/widget/xyz" } }
                ] } },
                { "id": "image_source", "settings": { "file": "C:/logo.png" } }
            ]
        });
        assert_eq!(count_browser_sources(&scene), 3);
    }

    #[test]
    fn les_sources_navigateur_sans_url_web_ne_sont_pas_comptees() {
        let scene = json!({
            "sources": [
                { "id": "browser_source", "settings": { "is_local_file": true,
                  "local_file": "C:/overlay/index.html", "url": "" } },
                { "id": "browser_source", "settings": {} },
                { "id": "browser_source" },
                { "id": "text_gdiplus", "settings": { "url": "https://pas-un-navigateur.test" } }
            ]
        });
        assert_eq!(count_browser_sources(&scene), 0);
    }
}
