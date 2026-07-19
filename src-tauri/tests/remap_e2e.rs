//! Test end-to-end de l'étape 4 du remappage matériel : diagnostic, choix
//! explicites, revalidation côté Rust puis application dans la copie
//! temporaire avant la bascule.
//!
//! Fichier d'intégration séparé exprès : les variables d'environnement
//! OWBS_* sont globales au processus, et ce binaire tourne dans son propre
//! processus — aucune interférence avec e2e.rs ou zip_slip.rs.

use owbs_lib::devices::{Famille, Peripherique};
use owbs_lib::{backup, remap, restore};
use std::fs;
use std::path::Path;

const FAKE_STREAM_KEY: &str = "live_9999_ULTRASECRETSTREAMKEY";

const ANCIEN_MICRO: &str = "{0.0.1.00000000}.{ancien-micro}";
const NOUVEAU_MICRO: &str = "{0.0.1.00000000}.{nouveau-micro}";
const ANCIEN_CASQUE: &str = "{0.0.0.00000000}.{ancien-casque}";
const ANCIENNE_WEBCAM: &str = r"Webcam C900:\\?\usb#22vid_1234#22ancien\global";
const NOUVELLE_WEBCAM: &str = r"Webcam C900:\\?\usb#22vid_1234#22autre-port\global";
const ID_PLUGIN_TIERS: &str = "{0.0.1.00000000}.{plugin-prive}";

/// Fausse config OBS source : trois familles de périphériques absents du PC
/// cible, une valeur spéciale `default`, une source de plugin tiers, et un
/// secret pour vérifier que la promesse de non-fuite tient toujours.
fn build_fake_config(root: &Path) {
    let scenes = root.join("basic").join("scenes");
    fs::create_dir_all(&scenes).unwrap();
    fs::write(
        scenes.join("Stream.json"),
        format!(
            r#"{{
  "name": "Stream",
  "AuxAudioDevice1": {{ "id": "wasapi_input_capture", "name": "Mic/Aux",
                       "settings": {{ "device_id": "default" }} }},
  "DesktopAudioDevice1": {{ "id": "wasapi_output_capture", "name": "Casque ancien",
                           "settings": {{ "device_id": "{ANCIEN_CASQUE}" }} }},
  "sources": [
    {{ "id": "wasapi_input_capture", "name": "Micro principal", "volume": 0.8,
      "settings": {{ "device_id": "{ANCIEN_MICRO}", "use_device_timing": true }} }},
    {{ "id": "wasapi_input_capture", "name": "Micro secondaire",
      "settings": {{ "device_id": "{ANCIEN_MICRO}" }} }},
    {{ "id": "dshow_input", "name": "Webcam",
      "settings": {{ "video_device_id": "{webcam}",
                    "last_video_device_id": "{webcam}" }} }},
    {{ "id": "plugin_tiers_source", "name": "Plugin tiers",
      "settings": {{ "device_id": "{ID_PLUGIN_TIERS}" }} }}
  ]
}}"#,
            webcam = ANCIENNE_WEBCAM.replace('\\', "\\\\"),
        ),
    )
    .unwrap();

    let profile = root.join("basic").join("profiles").join("Principal");
    fs::create_dir_all(&profile).unwrap();
    fs::write(
        profile.join("service.json"),
        format!(
            r#"{{ "type": "rtmp_common", "settings": {{ "service": "Twitch", "key": "{FAKE_STREAM_KEY}" }} }}"#
        ),
    )
    .unwrap();
    fs::write(root.join("global.ini"), "[General]\nFirstRun=false\n").unwrap();
}

/// Inventaire du PC cible : un remplaçant par famille, aucun des anciens
/// identifiants. Écrit dans un fichier pour OWBS_DEVICES_JSON — la
/// configuration et les périphériques réels ne sont jamais consultés.
fn inventaire_cible() -> Vec<Peripherique> {
    vec![
        Peripherique {
            famille: Famille::EntreeAudio,
            id: NOUVEAU_MICRO.to_string(),
            nom: "HyperX QuadCast".to_string(),
        },
        Peripherique {
            famille: Famille::SortieAudio,
            id: "{0.0.0.00000000}.{nouveau-casque}".to_string(),
            nom: "Casque USB".to_string(),
        },
        Peripherique {
            famille: Famille::Video,
            id: NOUVELLE_WEBCAM.to_string(),
            nom: "Webcam C900".to_string(),
        },
    ]
}

#[test]
fn remappage_de_bout_en_bout() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // --- Machine source : sauvegarde ---
    let config_src = root.join("obs-studio-src");
    build_fake_config(&config_src);
    let backup_file = root.join("remap.obsbackup");
    backup::create(&config_src, None, Some("32.0.0".to_string()), &backup_file, |_| {}).unwrap();
    let archive_avant = fs::read(&backup_file).unwrap();

    // --- Machine cible : bac à sable + inventaire factice ---
    let sandbox = root.join("machine2");
    let config_dst = sandbox.join("obs-studio");
    let devices_json = root.join("devices.json");
    fs::write(&devices_json, serde_json::to_string(&inventaire_cible()).unwrap()).unwrap();
    std::env::set_var("OWBS_CONFIG_DIR", &config_dst);
    std::env::set_var("OWBS_ASSETS_DIR", sandbox.join("OBS-Backup-Assets"));
    std::env::set_var("OWBS_INSTALL_DIR", sandbox.join("obs-install"));
    std::env::set_var("OWBS_OBS_VERSION", "32.0.0");
    std::env::set_var("OWBS_DEVICES_JSON", &devices_json);

    // --- Phase d'aperçu : diagnostic en lecture seule ---
    let rapport = remap::analyser(&backup_file, inventaire_cible()).unwrap();
    assert_eq!(rapport.a_confirmer.len(), 3, "{:#?}", rapport.a_confirmer);
    // `default` reste une référence valide, jamais proposée au remappage.
    assert_eq!(rapport.references_valides, 1);
    assert!(rapport
        .a_confirmer
        .iter()
        .all(|a| !a.ancien_id.contains("plugin-prive")));

    // L'utilisateur remplace le micro et la webcam, laisse le casque inchangé.
    let choix = vec![
        remap::Choix {
            famille: Famille::EntreeAudio,
            ancien_id: ANCIEN_MICRO.to_string(),
            nouveau_id: NOUVEAU_MICRO.to_string(),
        },
        remap::Choix {
            famille: Famille::Video,
            ancien_id: ANCIENNE_WEBCAM.to_string(),
            nouveau_id: NOUVELLE_WEBCAM.to_string(),
        },
    ];

    // --- Choix invalide : refusé avant la moindre écriture ---
    let mauvais = vec![remap::Choix {
        famille: Famille::EntreeAudio,
        ancien_id: ANCIEN_MICRO.to_string(),
        nouveau_id: "{0.0.1.00000000}.{debranche-entre-temps}".to_string(),
    }];
    let e = restore::restore(&backup_file, &mauvais, |_| {})
        .expect_err("un périphérique absent de l'inventaire doit être refusé");
    assert!(e.contains("plus disponible"), "{e}");
    assert!(
        !config_dst.exists() && !sandbox.exists(),
        "le refus doit intervenir avant toute écriture"
    );

    // Une ancienne référence qui n'était pas proposée par l'aperçu ne peut
    // pas être injectée par un appel Tauri fabriqué. `default` est valide et
    // doit donc rester impossible à remapper.
    let ancien_non_autorise = vec![remap::Choix {
        famille: Famille::EntreeAudio,
        ancien_id: "default".to_string(),
        nouveau_id: NOUVEAU_MICRO.to_string(),
    }];
    let e = restore::restore(&backup_file, &ancien_non_autorise, |_| {})
        .expect_err("une référence valide ne doit pas pouvoir être remappée");
    assert!(e.contains("ne fait pas partie"), "{e}");
    assert!(
        !config_dst.exists() && !sandbox.exists(),
        "le refus doit intervenir avant toute écriture"
    );

    // --- Restauration avec les bons choix ---
    let result = restore::restore(&backup_file, &choix, |_| {}).unwrap();
    assert_eq!(result.scene_collections, 1);
    assert_eq!(result.sources_remappees, 3, "2 sources micro + 1 webcam");

    let scene_text =
        fs::read_to_string(config_dst.join("basic").join("scenes").join("Stream.json")).unwrap();
    let scene: serde_json::Value = serde_json::from_str(&scene_text).unwrap();
    let sources = scene["sources"].as_array().unwrap();

    // Les deux sources micro pointent vers le nouveau périphérique, leurs
    // autres réglages sont conservés.
    assert_eq!(sources[0]["settings"]["device_id"], NOUVEAU_MICRO);
    assert_eq!(sources[0]["settings"]["use_device_timing"], true);
    assert_eq!(sources[0]["volume"], 0.8);
    assert_eq!(sources[1]["settings"]["device_id"], NOUVEAU_MICRO);
    // La webcam est réécrite sur ses deux champs miroir.
    assert_eq!(sources[2]["settings"]["video_device_id"], NOUVELLE_WEBCAM);
    assert_eq!(sources[2]["settings"]["last_video_device_id"], NOUVELLE_WEBCAM);
    // La source du plugin tiers est strictement intacte.
    assert_eq!(sources[3]["settings"]["device_id"], ID_PLUGIN_TIERS);
    // Laissé inchangé par l'utilisateur : l'ancien casque reste tel quel.
    assert_eq!(scene["DesktopAudioDevice1"]["settings"]["device_id"], ANCIEN_CASQUE);
    // La valeur spéciale `default` n'est jamais réécrite.
    assert_eq!(scene["AuxAudioDevice1"]["settings"]["device_id"], "default");

    // Promesse de non-fuite toujours tenue après remappage.
    assert!(!scene_text.contains(FAKE_STREAM_KEY));
    let service_text = fs::read_to_string(
        config_dst
            .join("basic")
            .join("profiles")
            .join("Principal")
            .join("service.json"),
    )
    .unwrap();
    assert!(!service_text.contains(FAKE_STREAM_KEY));

    // L'archive originale est inchangée au bit près, et aucun dossier
    // temporaire ne subsiste après la bascule.
    assert_eq!(archive_avant, fs::read(&backup_file).unwrap());
    let orphelins: Vec<String> = fs::read_dir(&sandbox)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("obs-studio.tmp-"))
        .collect();
    assert!(orphelins.is_empty(), "dossiers temporaires restants : {orphelins:?}");
}
