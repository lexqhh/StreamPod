//! Test end-to-end : backup → restore sur une fausse configuration OBS.
//! Vérifie notamment qu'aucune clé de stream ne fuit dans l'archive.

use streampod_lib::{backup, restore};
use std::fs;
use std::io::Read;
use std::path::Path;

const FAKE_STREAM_KEY: &str = "live_9999_ULTRASECRETSTREAMKEY";
const FAKE_TOKEN: &str = "oauthTOKENsecret123";
const FAKE_WS_PASSWORD: &str = "WSPASS_ULTRASECRET";
const FAKE_DB_TOKEN: &str = "SQLITE_TOKEN_ULTRASECRET";

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

    // Plugin tiers stockant ses secrets dans des formats opaques : seuls les
    // .json et .ini assainis sont archivés sous plugin_config/, le reste est
    // exclu (liste blanche), jamais copié brut.
    let exotique = root.join("plugin_config").join("plugin-exotique");
    fs::create_dir_all(&exotique).unwrap();
    fs::write(
        exotique.join("tokens.sqlite"),
        format!("SQLITE-BINAIRE {FAKE_DB_TOKEN}"),
    )
    .unwrap();
    fs::write(
        exotique.join("credentials.yaml"),
        format!("token: {FAKE_DB_TOKEN}\n"),
    )
    .unwrap();

    let browser = root.join("plugin_config").join("obs-browser");
    fs::create_dir_all(&browser).unwrap();
    fs::write(browser.join("cookies.sqlite"), "COOKIE-TWITCH-SESSION").unwrap();

    let logs = root.join("logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("2026-07-16.txt"), "log inutile").unwrap();

    // Marqueur laissé par OBS après un arrêt non propre.
    let sentinel = root.join(".sentinel");
    fs::create_dir_all(&sentinel).unwrap();
    fs::write(sentinel.join("run_0001"), "").unwrap();
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
        || false,
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
    assert!(
        warnings
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("plugin-exotique")),
        "l'exclusion des fichiers de plugin au format non pris en charge doit être signalée"
    );
    assert_eq!(summary.scene_collections, 1);
    assert_eq!(summary.profiles, 1);
    assert_eq!(summary.plugins, 1, "seul le plugin tiers doit être inclus");
    assert_eq!(summary.assets, 1);
    assert!(
        !backup_file.with_extension("obsbackup.tmp").exists(),
        "le fichier temporaire doit avoir été basculé vers la destination finale"
    );

    // --- Inspection de l'archive : aucun secret, exclusions respectées ---
    let mut zip = zip::ZipArchive::new(fs::File::open(&backup_file).unwrap()).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(names.contains(&"manifest.json".to_string()));
    assert!(
        !names.iter().any(|n| n.starts_with("plugins/")),
        "les DLL de plugins ne sont plus archivées (jamais restaurées : seul le manifest liste les plugins)"
    );
    assert!(!names.iter().any(|n| n.contains("win-capture")));
    assert!(!names.iter().any(|n| n.starts_with("config/logs/")));
    assert!(
        !names.iter().any(|n| n.starts_with("config/.sentinel")),
        "le marqueur d'arrêt non propre déclencherait le mode sans échec sur le PC cible"
    );
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
    assert!(
        !names
            .iter()
            .any(|n| n.contains("plugin-exotique") || n.ends_with(".sqlite") || n.ends_with(".yaml")),
        "sous plugin_config/, seuls les .json et .ini assainis sont archivés (liste blanche)"
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
        assert!(
            !text.contains(FAKE_DB_TOKEN),
            "le token du fichier de plugin opaque a fui dans {}",
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
    fs::remove_dir_all(
        install_dst
            .join("data")
            .join("obs-plugins")
            .join("move-transition"),
    )
    .unwrap(); // ... ni son dossier data

    std::env::set_var("STREAMPOD_CONFIG_DIR", &config_dst);
    std::env::set_var("STREAMPOD_ASSETS_DIR", &assets_dst);
    std::env::set_var("STREAMPOD_INSTALL_DIR", &install_dst);
    std::env::set_var("STREAMPOD_OBS_VERSION", "31.1.0"); // même version majeure

    let result = restore::restore(&backup_file, &[], |_| {}, || false).unwrap();
    assert_eq!(result.scene_collections, 1);
    assert_eq!(result.assets_restored, 1);
    assert_eq!(result.plugins_status, "manual");
    assert!(result.previous_config_backup.is_none());

    // La config est en place.
    assert!(config_dst.join("global.ini").is_file());
    assert!(!config_dst.join(".sentinel").exists());
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

    // Le plugin tiers n'est JAMAIS copié dans l'installation OBS de la
    // machine 2, même à version majeure identique : les DLL d'une archive
    // sont du code non fiable, elles sont seulement listées pour
    // réinstallation manuelle.
    assert!(!install_dst
        .join("obs-plugins")
        .join("64bit")
        .join("move-transition.dll")
        .exists());
    assert!(!install_dst
        .join("data")
        .join("obs-plugins")
        .join("move-transition")
        .join("locale.ini")
        .exists());
    assert_eq!(result.plugins, vec!["move-transition".to_string()]);

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
    let result2 = restore::restore(&backup_file, &[], |_| {}, || false).unwrap();
    let bak = result2.previous_config_backup.expect("copie de sécurité attendue");
    assert!(Path::new(&bak).join("global.ini").is_file());
}

/// Étape 2 du remappage matériel : le diagnostic est strictement en lecture
/// seule - il détecte les périphériques absents sans extraire, sans créer de
/// dossier temporaire, sans restaurer d'asset et sans modifier l'archive.
#[test]
fn diagnostic_remappage_en_lecture_seule() {
    use streampod_lib::devices::{Famille, Peripherique};
    use streampod_lib::remap;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // --- Fausse machine source : scènes avec périphériques + audio global ---
    let config_src = root.join("obs-studio-src");
    let scenes = config_src.join("basic").join("scenes");
    fs::create_dir_all(&scenes).unwrap();
    fs::write(
        scenes.join("Stream.json"),
        r#"{
  "name": "Stream",
  "AuxAudioDevice1": { "id": "wasapi_input_capture", "name": "Mic/Aux",
                       "settings": { "device_id": "default" } },
  "DesktopAudioDevice1": { "id": "wasapi_output_capture", "name": "Casque ancien",
                           "settings": { "device_id": "{0.0.0.00000000}.{dead-beef}" } },
  "sources": [
    { "id": "wasapi_input_capture", "name": "Micro principal",
      "settings": { "device_id": "{0.0.1.00000000}.{ancien-micro}" } },
    { "id": "wasapi_input_capture", "name": "Micro secondaire",
      "settings": { "device_id": "{0.0.1.00000000}.{ancien-micro}" } },
    { "id": "wasapi_input_capture", "name": "Micro encore branché",
      "settings": { "device_id": "{0.0.1.00000000}.{micro-valide}" } },
    { "id": "dshow_input", "name": "Webcam",
      "settings": { "video_device_id": "Webcam C900:\\\\?\\usb#22vid_1234#22{65e8773d}\\global",
                    "last_video_device_id": "Webcam C900:\\\\?\\usb#22vid_1234#22{65e8773d}\\global" } },
    { "id": "plugin_tiers_source", "name": "Plugin tiers",
      "settings": { "device_id": "{0.0.1.00000000}.{plugin-prive}" } }
  ]
}"#,
    )
    .unwrap();
    fs::write(config_src.join("global.ini"), "[General]\nFirstRun=false\n").unwrap();

    // --- Sauvegarde ---
    let backup_file = root.join("diag.obsbackup");
    backup::create(&config_src, None, Some("32.1.2".to_string()), &backup_file, |_| {}, || false).unwrap();
    let archive_avant = fs::read(&backup_file).unwrap();

    // --- Inventaire cible factice : le micro « valide » existe encore, la
    // webcam a un nouveau chemin matériel, et un remplaçant existe par
    // famille. La configuration réelle de la machine n'est jamais consultée
    // (aucune variable STREAMPOD_* n'est définie dans ce test). ---
    let inventaire = vec![
        Peripherique {
            famille: Famille::EntreeAudio,
            id: "{0.0.1.00000000}.{micro-valide}".to_string(),
            nom: "Micro encore branché".to_string(),
        },
        Peripherique {
            famille: Famille::EntreeAudio,
            id: "{0.0.1.00000000}.{nouveau-micro}".to_string(),
            nom: "HyperX QuadCast".to_string(),
        },
        Peripherique {
            famille: Famille::SortieAudio,
            id: "{0.0.0.00000000}.{nouveau-casque}".to_string(),
            nom: "Casque USB".to_string(),
        },
        Peripherique {
            famille: Famille::Video,
            id: r"Webcam C900:\\?\usb#22vid_1234#22autre-port#22{65e8773d}\global".to_string(),
            nom: "Webcam C900".to_string(),
        },
    ];

    let rapport = remap::analyser(&backup_file, inventaire).unwrap();

    // Trois associations à confirmer : micro absent (regroupé), casque
    // absent (audio global), webcam au chemin changé.
    assert_eq!(rapport.a_confirmer.len(), 3, "{:#?}", rapport.a_confirmer);
    let micro = rapport
        .a_confirmer
        .iter()
        .find(|a| a.ancien_id.contains("ancien-micro"))
        .expect("micro absent attendu");
    assert_eq!(micro.famille, Famille::EntreeAudio);
    assert_eq!(micro.occurrences, 2, "les 2 sources partagent une décision");
    let casque = rapport
        .a_confirmer
        .iter()
        .find(|a| a.ancien_id.contains("dead-beef"))
        .expect("sortie audio globale absente attendue");
    assert_eq!(casque.famille, Famille::SortieAudio);
    assert_eq!(casque.ancien_nom, "Casque ancien");
    let webcam = rapport
        .a_confirmer
        .iter()
        .find(|a| a.famille == Famille::Video)
        .expect("webcam au chemin changé attendue");
    assert_eq!(webcam.ancien_nom, "Webcam C900");

    // `default` et le micro encore branché restent valides, sans question.
    assert_eq!(rapport.references_valides, 2);
    // La source du plugin tiers n'est jamais analysée.
    assert!(rapport
        .a_confirmer
        .iter()
        .all(|a| !a.ancien_id.contains("plugin-prive")));

    // Lecture seule : archive inchangée au bit près, aucun dossier
    // temporaire de restauration, aucun asset restauré.
    assert_eq!(archive_avant, fs::read(&backup_file).unwrap());
    let reste: Vec<String> = walkdir_noms(root);
    assert!(
        reste.iter().all(|n| !n.contains("obs-studio.tmp-")),
        "dossier temporaire créé : {reste:?}"
    );
    assert!(!root.join("OBS-Backup-Assets").exists());
}

#[test]
fn backup_refuse_destination_dans_le_dossier_de_config() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let asset = root.join("assets-src").join("overlay.png");
    fs::create_dir_all(asset.parent().unwrap()).unwrap();
    fs::write(&asset, b"FAUX-PNG").unwrap();
    let config_src = root.join("obs-studio");
    build_fake_config(&config_src, &asset);

    // Sinon l'archive serait ramassée par le parcours du dossier de config
    // et se lirait elle-même pendant qu'elle grossit (disque saturé).
    for destination in [
        config_src.join("piege.obsbackup"),
        config_src.join("basic").join("piege.obsbackup"),
    ] {
        let err = backup::create(&config_src, None, None, &destination, |_| {}, || false)
            .expect_err("une destination dans le dossier de config doit être refusée");
        assert!(err.contains("dossier de configuration"), "{err}");
        assert!(!destination.exists(), "aucun fichier ne doit être créé");
    }

    // Une destination ailleurs reste acceptée.
    backup::create(&config_src, None, None, &root.join("ok.obsbackup"), |_| {}, || false).unwrap();
}

#[test]
fn backup_echoue_sans_laisser_de_fichier_incomplet() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let asset = root.join("assets-src").join("overlay.png");
    fs::create_dir_all(asset.parent().unwrap()).unwrap();
    fs::write(&asset, b"FAUX-PNG").unwrap();
    let config_src = root.join("obs-studio");
    build_fake_config(&config_src, &asset);
    // Un service.json corrompu fait échouer la sauvegarde en cours de route.
    fs::write(
        config_src
            .join("basic")
            .join("profiles")
            .join("Principal")
            .join("service.json"),
        "PAS-DU-JSON",
    )
    .unwrap();

    let destination = root.join("echec.obsbackup");
    backup::create(&config_src, None, None, &destination, |_| {}, || false)
        .expect_err("un service.json corrompu doit faire échouer la sauvegarde");
    assert!(
        !destination.exists(),
        "aucune archive incomplète ne doit rester à la destination"
    );
    assert!(
        !destination.with_extension("obsbackup.tmp").exists(),
        "le fichier temporaire doit être nettoyé après un échec"
    );
}

/// Liste les noms de tous les fichiers et dossiers sous `root`.
fn walkdir_noms(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            out.push(entry.file_name().to_string_lossy().into_owned());
            if entry.path().is_dir() {
                stack.push(entry.path());
            }
        }
    }
    out
}
