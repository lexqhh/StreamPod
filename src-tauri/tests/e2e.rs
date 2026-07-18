//! Test end-to-end : backup → restore sur une fausse configuration OBS.
//! Vérifie notamment qu'aucune clé de stream ne fuit dans l'archive.

use owbs_lib::{backup, restore};
use std::fs;
use std::io::Read;
use std::path::Path;

const FAKE_STREAM_KEY: &str = "live_9999_ULTRASECRETSTREAMKEY";
const FAKE_TOKEN: &str = "oauthTOKENsecret123";
const FAKE_WS_PASSWORD: &str = "WSPASS_ULTRASECRET";

/// Construit une fausse arborescence %APPDATA%\obs-studio.
fn build_fake_config(root: &Path, asset: &Path) {
    let scenes = root.join("basic").join("scenes");
    fs::create_dir_all(&scenes).unwrap();
    let asset_json = asset.to_string_lossy().replace('\\', "/");
    fs::write(
        scenes.join("Ma Collection.json"),
        format!(
            r#"{{
  "name": "Ma Collection",
  "sources": [
    {{ "id": "image_source", "settings": {{ "file": "{asset_json}" }} }},
    {{ "id": "ffmpeg_source", "settings": {{ "local_file": "C:/introuvable/video.mp4" }} }},
    {{ "hotkeys": {{ "key": "OBS_KEY_F1" }} }}
  ]
}}"#
        ),
    )
    .unwrap();

    let profile = root.join("basic").join("profiles").join("Principal");
    fs::create_dir_all(&profile).unwrap();
    fs::write(
        profile.join("basic.ini"),
        format!("[General]\nName=Principal\n[Twitch]\nRefreshToken={FAKE_TOKEN}\nDockState=ok\n"),
    )
    .unwrap();
    fs::write(
        profile.join("service.json"),
        format!(
            r#"{{ "type": "rtmp_common", "settings": {{ "service": "Twitch", "server": "auto", "key": "{FAKE_STREAM_KEY}" }} }}"#
        ),
    )
    .unwrap();

    // OBS garde des copies .bak qui contiennent les mêmes secrets.
    fs::write(
        profile.join("service.json.bak"),
        format!(r#"{{ "settings": {{ "key": "{FAKE_STREAM_KEY}" }} }}"#),
    )
    .unwrap();

    fs::write(root.join("global.ini"), "[General]\nFirstRun=false\n").unwrap();

    let ws = root.join("plugin_config").join("obs-websocket");
    fs::create_dir_all(&ws).unwrap();
    // Sentinelle en surface ET imbriquée pour verrouiller la récursion.
    fs::write(
        ws.join("config.json"),
        format!(
            r#"{{"server_enabled":true,"server_password":"{FAKE_WS_PASSWORD}","auth":{{"server_password":"{FAKE_WS_PASSWORD}"}}}}"#
        ),
    )
    .unwrap();

    // JSON de plugin corrompu contenant un secret : il doit être exclu de
    // l'archive (jamais copié tel quel) avec un avertissement.
    let broken = root.join("plugin_config").join("plugin-casse");
    fs::create_dir_all(&broken).unwrap();
    fs::write(
        broken.join("config.json"),
        format!(r#"{{"password":"{FAKE_WS_PASSWORD}" PAS-DU-JSON"#),
    )
    .unwrap();

    let browser = root.join("plugin_config").join("obs-browser");
    fs::create_dir_all(&browser).unwrap();
    fs::write(browser.join("cookies.sqlite"), "COOKIE-TWITCH-SESSION").unwrap();

    let logs = root.join("logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("2026-07-16.txt"), "log inutile").unwrap();
}

/// Construit un faux dossier d'installation OBS avec un plugin tiers.
fn build_fake_install(root: &Path) {
    let plugins = root.join("obs-plugins").join("64bit");
    fs::create_dir_all(&plugins).unwrap();
    fs::write(plugins.join("win-capture.dll"), "OFFICIEL").unwrap();
    fs::write(plugins.join("move-transition.dll"), "PLUGIN-TIERS").unwrap();
    let data = root.join("data").join("obs-plugins").join("move-transition");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("locale.ini"), "[fr-FR]\nNom=Move").unwrap();
}

#[test]
fn backup_puis_restore_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // --- Préparation de la fausse machine "source" ---
    let asset = root.join("assets-src").join("overlay.png");
    fs::create_dir_all(asset.parent().unwrap()).unwrap();
    fs::write(&asset, b"FAUX-PNG").unwrap();

    let config_src = root.join("obs-studio");
    build_fake_config(&config_src, &asset);
    let install_src = root.join("obs-install-src");
    build_fake_install(&install_src);

    // --- Sauvegarde ---
    let backup_file = root.join("ma-sauvegarde.obsbackup");
    let warnings = std::sync::Mutex::new(Vec::<String>::new());
    let summary = backup::create(
        &config_src,
        Some(&install_src),
        Some("31.0.2".to_string()),
        &backup_file,
        |p| {
            if p.step == "warning" {
                warnings.lock().unwrap().push(p.message);
            }
        },
    )
    .unwrap();
    assert!(
        warnings
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("plugin-casse")),
        "l'exclusion du JSON de plugin invalide doit être signalée à l'utilisateur"
    );
    assert_eq!(summary.scene_collections, 1);
    assert_eq!(summary.profiles, 1);
    assert_eq!(summary.plugins, 1, "seul le plugin tiers doit être inclus");
    assert_eq!(summary.assets, 1);

    // --- Inspection de l'archive : aucun secret, exclusions respectées ---
    let mut zip = zip::ZipArchive::new(fs::File::open(&backup_file).unwrap()).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(names.contains(&"manifest.json".to_string()));
    assert!(names.contains(&"plugins/64bit/move-transition.dll".to_string()));
    assert!(!names.iter().any(|n| n.contains("win-capture")));
    assert!(!names.iter().any(|n| n.starts_with("config/logs/")));
    assert!(
        !names.iter().any(|n| n.contains("obs-browser")),
        "les cookies des docks navigateur ne doivent pas être sauvegardés"
    );
    assert!(names.iter().any(|n| n.starts_with("assets/0/")));
    assert!(
        names.contains(&"config/plugin_config/obs-websocket/config.json".to_string()),
        "le config.json d'obs-websocket doit être présent (assaini, pas exclu)"
    );
    assert!(
        !names.iter().any(|n| n.contains("plugin-casse")),
        "un JSON de plugin invalide ne doit jamais être copié tel quel dans l'archive"
    );

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).unwrap();
        let text = String::from_utf8_lossy(&content);
        assert!(
            !text.contains(FAKE_STREAM_KEY),
            "la clé de stream a fui dans {}",
            entry.name()
        );
        assert!(
            !text.contains(FAKE_TOKEN),
            "le token OAuth a fui dans {}",
            entry.name()
        );
        assert!(
            !text.contains(FAKE_WS_PASSWORD),
            "le mot de passe obs-websocket a fui dans {}",
            entry.name()
        );
    }

    // Le config.json d'obs-websocket est assaini (mot de passe retiré) mais
    // conserve les autres champs.
    let mut ws_entry = zip
        .by_name("config/plugin_config/obs-websocket/config.json")
        .unwrap();
    let mut ws_text = String::new();
    ws_entry.read_to_string(&mut ws_text).unwrap();
    assert!(ws_text.contains("server_enabled"));
    assert!(!ws_text.contains("server_password"));
    drop(ws_entry);

    // La touche de raccourci "key" des scènes ne doit PAS être supprimée
    // (seul service.json est nettoyé).
    let mut scene_entry = zip.by_name("config/basic/scenes/Ma Collection.json").unwrap();
    let mut scene_text = String::new();
    scene_entry.read_to_string(&mut scene_text).unwrap();
    assert!(scene_text.contains("OBS_KEY_F1"));
    drop(scene_entry);

    // --- Restauration sur la fausse machine "destination" ---
    let config_dst = root.join("machine2").join("obs-studio");
    let assets_dst = root.join("machine2").join("OBS-Backup-Assets");
    let install_dst = root.join("machine2").join("obs-install");
    build_fake_install(&install_dst); // OBS "installé" sur la machine 2
    fs::remove_file(
        install_dst
            .join("obs-plugins")
            .join("64bit")
            .join("move-transition.dll"),
    )
    .unwrap(); // ... mais sans le plugin tiers

    std::env::set_var("OWBS_CONFIG_DIR", &config_dst);
    std::env::set_var("OWBS_ASSETS_DIR", &assets_dst);
    std::env::set_var("OWBS_INSTALL_DIR", &install_dst);
    std::env::set_var("OWBS_OBS_VERSION", "31.1.0"); // même version majeure

    let result = restore::restore(&backup_file, |_| {}).unwrap();
    assert_eq!(result.scene_collections, 1);
    assert_eq!(result.assets_restored, 1);
    assert_eq!(result.plugins_status, "copied");
    assert!(result.previous_config_backup.is_none());

    // La config est en place.
    assert!(config_dst.join("global.ini").is_file());
    assert!(config_dst
        .join("basic")
        .join("profiles")
        .join("Principal")
        .join("service.json")
        .is_file());

    // L'asset est restauré et le JSON de scènes pointe vers lui.
    let restored_asset = assets_dst.join("0").join("overlay.png");
    assert!(restored_asset.is_file());
    let scene_text = fs::read_to_string(
        config_dst.join("basic").join("scenes").join("Ma Collection.json"),
    )
    .unwrap();
    let expected = restored_asset.to_string_lossy().replace('\\', "/");
    assert!(
        scene_text.contains(&expected),
        "le chemin d'asset n'a pas été réécrit : {scene_text}"
    );

    // Le plugin tiers a été copié dans l'installation OBS de la machine 2.
    assert!(install_dst
        .join("obs-plugins")
        .join("64bit")
        .join("move-transition.dll")
        .is_file());
    assert!(install_dst
        .join("data")
        .join("obs-plugins")
        .join("move-transition")
        .join("locale.ini")
        .is_file());

    // Aucun secret dans la config restaurée.
    let service_text = fs::read_to_string(
        config_dst
            .join("basic")
            .join("profiles")
            .join("Principal")
            .join("service.json"),
    )
    .unwrap();
    assert!(!service_text.contains(FAKE_STREAM_KEY));

    let ws_config_text = fs::read_to_string(
        config_dst
            .join("plugin_config")
            .join("obs-websocket")
            .join("config.json"),
    )
    .unwrap();
    assert!(!ws_config_text.contains(FAKE_WS_PASSWORD));

    // --- Seconde restauration : l'ancienne config est mise de côté ---
    std::thread::sleep(std::time::Duration::from_millis(1100)); // horodatage différent
    let result2 = restore::restore(&backup_file, |_| {}).unwrap();
    let bak = result2.previous_config_backup.expect("copie de sécurité attendue");
    assert!(Path::new(&bak).join("global.ini").is_file());
}
