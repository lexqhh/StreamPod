pub mod backup;
pub mod copies;
pub mod devices;
pub mod obs;
pub mod polices;
pub mod remap;
pub mod restore;
pub mod sanitize;
pub mod scenes;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager};

/// Demande d'annulation de l'opération longue en cours (sauvegarde ou
/// restauration). Une seule opération à la fois côté UI, un simple flag
/// global suffit ; remis à zéro au démarrage de chaque opération.
static ANNULATION_DEMANDEE: AtomicBool = AtomicBool::new(false);

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
            obs::plugins_dir().as_deref(),
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
            return Err(obs::OBS_RUNNING_MSG.to_string());
        }
        let config = obs::config_dir().ok_or(NO_CONFIG_MSG)?;
        backup::create_avec_garde(
            &config,
            obs::install_dir().as_deref(),
            obs::plugins_dir().as_deref(),
            obs::installed_version(),
            Path::new(&output_path),
            |p| {
                let _ = app.emit("streampod://progress", &p);
            },
            || ANNULATION_DEMANDEE.load(Ordering::Relaxed),
            obs::is_running,
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
        if obs::is_running() {
            return Err(obs::OBS_RUNNING_MSG.to_string());
        }
        restore::restore_avec_garde(
            Path::new(&backup_path),
            &choix,
            |p| {
                let _ = app.emit("streampod://progress", &p);
            },
            || ANNULATION_DEMANDEE.load(Ordering::Relaxed),
            obs::is_running,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Premier argument désignant un `.obsbackup` existant (double-clic sur un
/// fichier associé à StreamPod).
fn sauvegarde_parmi<S: AsRef<str>>(args: impl IntoIterator<Item = S>) -> Option<String> {
    args.into_iter()
        .map(|a| a.as_ref().to_string())
        .find(|a| a.to_lowercase().ends_with(".obsbackup") && Path::new(a).is_file())
}

/// Sauvegarde passée en argument au lancement, à ouvrir directement.
#[tauri::command]
fn fichier_au_lancement() -> Option<String> {
    sauvegarde_parmi(std::env::args().skip(1))
}

#[tauri::command]
async fn lister_copies_securite() -> Result<Vec<copies::CopieSecurite>, String> {
    tauri::async_runtime::spawn_blocking(copies::lister)
        .await
        .map_err(|e| e.to_string())?
}

/// Retour à une copie de sécurité : refusé si OBS tourne, rollback garanti.
#[tauri::command]
async fn revenir_a_copie(chemin: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || copies::revenir_a(&chemin, obs::is_running))
        .await
        .map_err(|e| e.to_string())?
}

/// Envoi d'une copie de sécurité à la corbeille (jamais de suppression définitive).
#[tauri::command]
async fn jeter_copie(chemin: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || copies::jeter(&chemin))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // En premier : une 2e ouverture (double-clic sur un autre
        // .obsbackup) est renvoyée à la fenêtre existante.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(fenetre) = app.get_webview_window("main") {
                let _ = fenetre.unminimize();
                let _ = fenetre.set_focus();
            }
            if let Some(chemin) = sauvegarde_parmi(argv.iter().skip(1)) {
                let _ = app.emit("streampod://ouvrir", chemin);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            detect_obs,
            cancel_operation,
            backup_preview,
            backup_create,
            restore_preview,
            remap_preview,
            restore_run,
            fichier_au_lancement,
            lister_copies_securite,
            revenir_a_copie,
            jeter_copie
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::sauvegarde_parmi;

    #[test]
    fn seul_un_obsbackup_existant_est_retenu() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("Mon OBS.OBSBACKUP");
        std::fs::write(&archive, "zip").unwrap();
        let archive = archive.to_string_lossy().into_owned();
        let absent = dir.path().join("absent.obsbackup");
        let args = [
            "--flag".to_string(),
            absent.to_string_lossy().into_owned(),
            archive.clone(),
        ];
        assert_eq!(sauvegarde_parmi(&args), Some(archive));
        assert_eq!(sauvegarde_parmi(["autre.txt"]), None);
    }
}
