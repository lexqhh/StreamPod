pub mod backup;
pub mod obs;
pub mod restore;
pub mod sanitize;
pub mod scenes;

use std::path::Path;
use tauri::Emitter;

const OBS_RUNNING_MSG: &str =
    "OBS est en cours d'exécution. Fermez OBS puis réessayez.";
const NO_CONFIG_MSG: &str =
    "Aucune configuration OBS trouvée sur cet ordinateur (dossier obs-studio introuvable). \
     OBS a-t-il déjà été lancé ici ?";

#[tauri::command]
fn detect_obs() -> obs::ObsInfo {
    obs::detect()
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
                let _ = app.emit("owbs://progress", &p);
            },
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

#[tauri::command]
async fn restore_run(
    app: tauri::AppHandle,
    backup_path: String,
) -> Result<restore::RestoreSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        restore::restore(Path::new(&backup_path), |p| {
            let _ = app.emit("owbs://progress", &p);
        })
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
            backup_preview,
            backup_create,
            restore_preview,
            restore_run
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
