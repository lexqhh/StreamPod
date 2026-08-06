pub mod backup;
pub mod devices;
pub mod obs;
pub mod remap;
pub mod restore;
pub mod sanitize;
pub mod scenes;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;

/// Demande d'annulation de l'opération longue en cours (sauvegarde ou
/// restauration). Une seule opération à la fois côté UI, un simple flag
/// global suffit ; remis à zéro au démarrage de chaque opération.
static ANNULATION_DEMANDEE: AtomicBool = AtomicBool::new(false);

const OBS_RUNNING_MSG: &str =
    "OBS est en cours d'exécution. Fermez OBS puis réessayez.";
const NO_CONFIG_MSG: &str =
    "Aucune configuration OBS trouvée sur cet ordinateur (dossier obs-studio introuvable). \
     OBS a-t-il déjà été lancé ici ?";

/// Appelée périodiquement par l'interface : asynchrone pour que la lecture du
/// registre et l'énumération des processus ne bloquent jamais le thread
/// principal, y compris pendant une sauvegarde.
#[tauri::command]
async fn detect_obs() -> Result<obs::ObsInfo, String> {
    tauri::async_runtime::spawn_blocking(obs::detect)
        .await
        .map_err(|e| e.to_string())
}

/// Demande l'annulation de la sauvegarde ou restauration en cours. Coopératif :
/// l'opération s'arrête à son prochain point de contrôle et nettoie ses
/// fichiers temporaires ; ignoré une fois la bascule de configuration engagée.
#[tauri::command]
fn cancel_operation() {
    ANNULATION_DEMANDEE.store(true, Ordering::Relaxed);
}

#[tauri::command]
async fn backup_preview() -> Result<backup::BackupPreview, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let config = obs::config_dir().ok_or(NO_CONFIG_MSG)?;
        backup::preview(
            &config,
            obs::install_dir().as_deref(),
            obs::installed_version(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn backup_create(
    app: tauri::AppHandle,
    output_path: String,
) -> Result<backup::BackupSummary, String> {
    ANNULATION_DEMANDEE.store(false, Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || {
        if obs::is_running() {
            return Err(OBS_RUNNING_MSG.to_string());
        }
        let config = obs::config_dir().ok_or(NO_CONFIG_MSG)?;
        backup::create(
            &config,
            obs::install_dir().as_deref(),
            obs::installed_version(),
            Path::new(&output_path),
            |p| {
                let _ = app.emit("streampod://progress", &p);
            },
            || ANNULATION_DEMANDEE.load(Ordering::Relaxed),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn restore_preview(backup_path: String) -> Result<restore::RestorePreview, String> {
    tauri::async_runtime::spawn_blocking(move || restore::preview(Path::new(&backup_path)))
        .await
        .map_err(|e| e.to_string())?
}

/// Diagnostic du remappage matériel : lecture seule de l'archive + inventaire
/// des périphériques de ce PC. Aucune extraction, aucun dossier temporaire.
#[tauri::command]
async fn remap_preview(backup_path: String) -> Result<remap::RemapReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let peripheriques = devices::inventaire()?;
        remap::analyser(Path::new(&backup_path), peripheriques)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Restauration complète. `choix` contient les remplacements de périphériques
/// confirmés à l'écran de remappage (vide si rien à remapper) : ils sont
/// revalidés côté Rust avant application.
#[tauri::command]
async fn restore_run(
    app: tauri::AppHandle,
    backup_path: String,
    choix: Vec<remap::Choix>,
) -> Result<restore::RestoreSummary, String> {
    ANNULATION_DEMANDEE.store(false, Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || {
        restore::restore(
            Path::new(&backup_path),
            &choix,
            |p| {
                let _ = app.emit("streampod://progress", &p);
            },
            || ANNULATION_DEMANDEE.load(Ordering::Relaxed),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            detect_obs,
            cancel_operation,
            backup_preview,
            backup_create,
            restore_preview,
            remap_preview,
            restore_run
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
