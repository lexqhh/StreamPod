//! Test de fumée sur la machine réelle (lecture seule sur la vraie config).
//! Lancé explicitement : cargo test --test real_machine -- --ignored
//!
//! - Sauvegarde la vraie configuration OBS vers un fichier temporaire.
//! - Vérifie qu'aucune clé de stream réelle ni cookie n'y figure.
//! - Restaure vers des dossiers temporaires (la vraie config n'est jamais
//!   modifiée grâce aux variables OWBS_*).

use owbs_lib::{backup, obs, restore};
use std::io::Read;

#[test]
#[ignore]
fn sauvegarde_reelle_puis_restauration_en_bac_a_sable() {
    let Some(config) = obs::config_dir() else {
        eprintln!("OBS non installé sur cette machine, test ignoré.");
        return;
    };
    let install = obs::install_dir();
    let version = obs::installed_version();
    println!("Config : {} | Install : {:?} | Version : {:?}", config.display(), install, version);

    // Les clés de stream réellement présentes sur le disque ne doivent pas
    // se retrouver dans l'archive.
    let mut real_keys: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(config.join("basic").join("profiles")) {
        for profile in entries.flatten() {
            let service = profile.path().join("service.json");
            let Ok(text) = std::fs::read_to_string(&service) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            if let Some(key) = value
                .pointer("/settings/key")
                .and_then(|k| k.as_str())
                .filter(|k| k.len() > 8)
            {
                real_keys.push(key.to_string());
            }
        }
    }
    println!("{} clé(s) de stream trouvée(s) dans la vraie config.", real_keys.len());

    let tmp = tempfile::tempdir().unwrap();
    let backup_file = tmp.path().join("smoke.obsbackup");

    let summary = backup::create(
        &config,
        install.as_deref(),
        version.clone(),
        &backup_file,
        |_| {},
        || false,
    )
    .unwrap();
    println!(
        "Sauvegarde : {} scènes, {} profils, {} plugins, {} assets, {} octets",
        summary.scene_collections, summary.profiles, summary.plugins, summary.assets, summary.file_size
    );

    // Inspection : pas de secrets, pas de cookies navigateur, pas de logs.
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&backup_file).unwrap()).unwrap();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let name = entry.name().to_string();
        assert!(!name.contains("obs-browser"), "cookies présents : {name}");
        assert!(!name.starts_with("config/logs/"), "logs présents : {name}");
        // Les secrets ne peuvent se cacher que dans les fichiers texte de
        // config (les assets binaires viennent d'ailleurs).
        if name.starts_with("config/") {
            let mut content = Vec::new();
            entry.read_to_end(&mut content).unwrap();
            let text = String::from_utf8_lossy(&content);
            for key in &real_keys {
                assert!(!text.contains(key.as_str()), "clé de stream trouvée dans {name}");
            }
        }
    }

    // Restauration en bac à sable.
    let sandbox = tmp.path().join("machine2");
    std::env::set_var("OWBS_CONFIG_DIR", sandbox.join("obs-studio"));
    std::env::set_var("OWBS_ASSETS_DIR", sandbox.join("assets"));
    let fake_install = sandbox.join("obs-install");
    std::fs::create_dir_all(fake_install.join("obs-plugins").join("64bit")).unwrap();
    std::env::set_var("OWBS_INSTALL_DIR", &fake_install);
    if let Some(v) = &version {
        std::env::set_var("OWBS_OBS_VERSION", v);
    }

    let result = restore::restore(&backup_file, &[], |_| {}, || false).unwrap();
    println!(
        "Restauration : {} scènes, {} assets, plugins = {} ({:?})",
        result.scene_collections, result.assets_restored, result.plugins_status, result.plugins
    );
    assert_eq!(result.scene_collections, summary.scene_collections);
    assert!(sandbox.join("obs-studio").join("global.ini").is_file()
        || sandbox.join("obs-studio").join("user.ini").is_file());
}

/// Énumération réelle des périphériques (lecture seule, aucune écriture).
/// Vérifie que les identifiants produits ont exactement le format qu'OBS
/// écrit dans ses JSON (voir docs/FORMATS-OBS.md).
#[test]
#[ignore]
fn inventaire_reel_des_peripheriques() {
    use owbs_lib::devices::{self, Famille};

    let inventaire = devices::inventaire_reel().unwrap();
    let entrees = inventaire.iter().filter(|p| p.famille == Famille::EntreeAudio).count();
    let sorties = inventaire.iter().filter(|p| p.famille == Famille::SortieAudio).count();
    let videos = inventaire.iter().filter(|p| p.famille == Famille::Video).count();
    println!("Inventaire : {entrees} entrée(s) audio, {sorties} sortie(s) audio, {videos} périphérique(s) vidéo");

    for p in &inventaire {
        // Pas d'identifiant réel dans la sortie du test : seuls les formats
        // sont vérifiés.
        match p.famille {
            Famille::EntreeAudio => assert!(
                p.id.starts_with("{0.0.1."),
                "format d'entrée audio inattendu pour « {} »", p.nom
            ),
            Famille::SortieAudio => assert!(
                p.id.starts_with("{0.0.0."),
                "format de sortie audio inattendu pour « {} »", p.nom
            ),
            Famille::Video => {
                assert!(
                    p.id.contains(':'),
                    "identifiant vidéo sans séparateur pour « {} »", p.nom
                );
                assert_eq!(
                    devices::nom_video(&p.id).as_deref(),
                    Some(p.nom.as_str()),
                    "le nom encodé doit correspondre au nom convivial"
                );
            }
        }
        assert!(!p.nom.is_empty());
    }
}
