//! Annulation coopérative : une sauvegarde ou une restauration annulée
//! s'arrête proprement, nettoie ses fichiers temporaires et ne touche ni la
//! configuration active ni le dossier d'assets définitif.

use streampod_lib::{backup, restore};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Construit une fausse configuration OBS minimale avec un asset.
fn build_fake_config(root: &Path, asset: &Path) {
    let scenes = root.join("basic").join("scenes");
    fs::create_dir_all(&scenes).unwrap();
    let asset_json = asset.to_string_lossy().replace('\\', "/");
    fs::write(
        scenes.join("Ma Collection.json"),
        format!(
            r#"{{ "name": "Ma Collection", "sources": [
  {{ "id": "image_source", "settings": {{ "file": "{asset_json}" }} }} ] }}"#
        ),
    )
    .unwrap();
    let profile = root.join("basic").join("profiles").join("Principal");
    fs::create_dir_all(&profile).unwrap();
    fs::write(profile.join("basic.ini"), "[General]\nName=Principal\n").unwrap();
    fs::write(root.join("global.ini"), "[General]\nFirstRun=false\n").unwrap();
}

/// Closure d'annulation qui devient vraie à partir du n-ième appel.
fn annule_apres(n: usize) -> impl Fn() -> bool {
    let compteur = AtomicUsize::new(0);
    move || compteur.fetch_add(1, Ordering::Relaxed) + 1 >= n
}

#[test]
fn backup_annule_ne_laisse_ni_archive_ni_temporaire() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let asset = root.join("assets-src").join("overlay.png");
    fs::create_dir_all(asset.parent().unwrap()).unwrap();
    fs::write(&asset, b"FAUX-PNG").unwrap();
    let config_src = root.join("obs-studio");
    build_fake_config(&config_src, &asset);

    let destination = root.join("annulee.obsbackup");
    let e = backup::create(&config_src, None, None, None, &destination, |_| {}, annule_apres(2))
        .expect_err("la sauvegarde annulée doit échouer");
    assert_eq!(e, backup::MSG_ANNULATION);
    assert!(!destination.exists(), "aucune archive ne doit être créée");
    assert!(
        !destination.with_extension("obsbackup.tmp").exists(),
        "le fichier temporaire doit être nettoyé après une annulation"
    );
}

#[test]
fn restore_annule_pendant_extraction_nettoie_et_preserve_le_disque() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Sauvegarde valide depuis une fausse machine source.
    let asset = root.join("assets-src").join("overlay.png");
    fs::create_dir_all(asset.parent().unwrap()).unwrap();
    fs::write(&asset, b"FAUX-PNG").unwrap();
    let config_src = root.join("obs-studio-src");
    build_fake_config(&config_src, &asset);
    let backup_file = root.join("sauvegarde.obsbackup");
    backup::create(&config_src, None, None, None, &backup_file, |_| {}, || false).unwrap();

    // Machine cible : bac à sable redirigé, jamais la vraie config.
    let sandbox = root.join("machine-cible");
    let config_dst = sandbox.join("obs-studio");
    std::env::set_var("STREAMPOD_CONFIG_DIR", &config_dst);
    std::env::set_var("STREAMPOD_ASSETS_DIR", sandbox.join("OBS-Backup-Assets"));
    std::env::set_var("STREAMPOD_INSTALL_DIR", sandbox.join("obs-install"));

    let e = restore::restore(&backup_file, &[], |_| {}, annule_apres(2))
        .expect_err("la restauration annulée doit échouer");
    assert_eq!(e, backup::MSG_ANNULATION);

    // Ni configuration installée, ni temporaire orphelin, ni asset déposé.
    assert!(!config_dst.exists(), "aucune configuration ne doit être mise en place");
    assert!(
        !sandbox.join("OBS-Backup-Assets").exists(),
        "le dossier d'assets définitif ne doit pas être touché"
    );
    let restes: Vec<String> = fs::read_dir(&sandbox)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp-"))
        .collect();
    assert!(restes.is_empty(), "temporaires non nettoyés : {restes:?}");
}
