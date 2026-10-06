pub mod backup;
pub mod copies;
pub mod devices;
pub mod maj;
pub mod obs;
pub mod polices;
pub mod remap;
pub mod restore;
pub mod sanitize;
pub mod scenes;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

/// Demande d'annulation de l'opération longue en cours (sauvegarde ou
/// restauration). Une seule opération à la fois côté UI, un simple flag
/// global suffit ; remis à zéro au démarrage de chaque opération.
static ANNULATION_DEMANDEE: AtomicBool = AtomicBool::new(false);

/// Sauvegarde, restauration ou retour à une copie en cours : une mise à jour
/// ne doit jamais quitter l'app au milieu.
static OPERATION_EN_COURS: AtomicBool = AtomicBool::new(false);

struct GardeOperation;

impl GardeOperation {
    fn prendre() -> Self {
        OPERATION_EN_COURS.store(true, Ordering::SeqCst);
        GardeOperation
    }
}

impl Drop for GardeOperation {
    fn drop(&mut self) {
        OPERATION_EN_COURS.store(false, Ordering::SeqCst);
    }
}

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
        let _garde = GardeOperation::prendre();
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
        let _garde = GardeOperation::prendre();
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
    tauri::async_runtime::spawn_blocking(move || {
        let _garde = GardeOperation::prendre();
        copies::revenir_a(&chemin, obs::is_running)
    })
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

/// Exécutable lancé : son chemin reste valable après le renommage en `.old`
/// d'une mise à jour portable.
fn exe_courant() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Mise à jour trouvée par la dernière vérification, appliquée au clic.
#[derive(Default)]
struct MajEnAttente(Mutex<Option<tauri_plugin_updater::Update>>);

#[derive(serde::Serialize)]
struct InfosMaj {
    version: String,
    notes: Option<String>,
    mode: maj::ModeExecution,
}

const URL_RELEASE: &str = "https://github.com/lexqhh/StreamPod/releases/latest";
const MSG_SIGNATURE: &str = "Signature de la mise à jour invalide : fichier refusé.";

/// `manuel` : « Rechercher maintenant ». Au démarrage, un échec réseau est
/// silencieux (`Ok(None)`) ; sur demande, il est signalé.
#[tauri::command]
async fn verifier_mise_a_jour(
    app: tauri::AppHandle,
    attente: tauri::State<'_, MajEnAttente>,
    manuel: bool,
) -> Result<Option<InfosMaj>, String> {
    if maj::desactivee_par_env() {
        return Ok(None);
    }
    let mode = exe_courant().map_or(maj::ModeExecution::Portable, |e| maj::mode_execution(&e));
    let mut builder = app.updater_builder();
    if mode == maj::ModeExecution::Portable {
        builder = builder.target(maj::CIBLE_PORTABLE);
    }
    let resultat = match builder.build() {
        Ok(updater) => updater.check().await.map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    let update =
        match resultat {
            Ok(u) => u,
            Err(_) if !manuel => return Ok(None),
            Err(_) => return Err(
                "Impossible de rechercher les mises à jour : vérifiez votre connexion à Internet."
                    .into(),
            ),
        };
    let infos = update.as_ref().map(|u| InfosMaj {
        version: u.version.clone(),
        notes: u.body.clone(),
        mode,
    });
    *attente.0.lock().map_err(|e| e.to_string())? = update;
    Ok(infos)
}

/// Télécharge (signature vérifiée par le plugin) puis installe : NSIS passif
/// et relance pour la version installée, remplacement de l'exe pour la
/// portable. Refusée pendant une opération longue.
#[tauri::command]
async fn appliquer_mise_a_jour(
    app: tauri::AppHandle,
    attente: tauri::State<'_, MajEnAttente>,
) -> Result<(), String> {
    if OPERATION_EN_COURS.load(Ordering::SeqCst) {
        return Err("Une opération est en cours : attendez sa fin avant de mettre à jour.".into());
    }
    let exe = exe_courant().ok_or("Emplacement de StreamPod introuvable.")?;
    let portable = maj::mode_execution(&exe) == maj::ModeExecution::Portable;
    if portable && !maj::dossier_inscriptible(exe.parent().unwrap_or(Path::new("."))) {
        return Err(maj::MSG_LECTURE_SEULE.into());
    }
    let update = attente
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .take()
        .ok_or("Aucune mise à jour à installer : relancez la recherche.")?;

    let mut recu = 0u64;
    let progression = |morceau: usize, total: Option<u64>| {
        recu += morceau as u64;
        let _ = app.emit(
            "streampod://progress",
            backup::Progress {
                step: "maj".into(),
                message: "Téléchargement de la nouvelle version…".into(),
                current: recu,
                total: total.unwrap_or(0),
            },
        );
    };
    let octets = update
        .download(progression, || {})
        .await
        .map_err(|e| match e {
            tauri_plugin_updater::Error::Minisign(_)
            | tauri_plugin_updater::Error::Base64(_)
            | tauri_plugin_updater::Error::SignatureUtf8(_)
            | tauri_plugin_updater::Error::SignedVersionMismatch { .. } => MSG_SIGNATURE.into(),
            e => format!("Téléchargement de la mise à jour impossible : {e}"),
        })?;

    if !portable {
        // Lance l'installateur NSIS en mode passif puis quitte ; il relance StreamPod.
        return update
            .install(octets)
            .map_err(|e| format!("Installation de la mise à jour impossible : {e}"));
    }
    let cle = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .unwrap_or_default()
        .to_string();
    let exe_maj = exe.clone();
    tauri::async_runtime::spawn_blocking(move || {
        maj::installer_portable(&exe_maj, &octets, &update.signature, &cle)
    })
    .await
    .map_err(|e| e.to_string())??;
    std::process::Command::new(&exe)
        .arg(maj::ARG_APRES_MAJ)
        .spawn()
        .map_err(|e| format!("Nouvelle version installée : relancez StreamPod ({e})."))?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn ouvrir_page_release(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(URL_RELEASE, None::<&str>)
        .map_err(|e| e.to_string())
}

fn dossier_donnees(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

#[tauri::command]
fn lire_reglage_maj(app: tauri::AppHandle) -> Result<maj::ReglageMaj, String> {
    Ok(maj::lire_reglage(&dossier_donnees(&app)?))
}

#[tauri::command]
fn ecrire_reglage_maj(app: tauri::AppHandle, reglage: maj::ReglageMaj) -> Result<(), String> {
    maj::ecrire_reglage(&dossier_donnees(&app)?, &reglage)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Avant l'instance unique : après une mise à jour portable, attendre que
    // l'ancien processus libère `<exe>.old`.
    if let Some(exe) = exe_courant() {
        let apres_maj = std::env::args().any(|a| a == maj::ARG_APRES_MAJ);
        maj::nettoyer_ancien(&exe, apres_maj);
    }
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(MajEnAttente::default())
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
            jeter_copie,
            verifier_mise_a_jour,
            appliquer_mise_a_jour,
            ouvrir_page_release,
            lire_reglage_maj,
            ecrire_reglage_maj
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
