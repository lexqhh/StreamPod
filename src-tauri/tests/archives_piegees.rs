//! Archives piégées au-delà du zip slip : exécutables et scripts, obs-websocket
//! ouvert, bombes de décompression, OBS lancé pendant l'opération.

use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::Path;
use streampod_lib::backup::{self, FORMAT_VERSION};
use streampod_lib::{obs, restore};

fn ecrire_archive(path: &Path, manifest: &Value, entries: &[(&str, &[u8])]) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("manifest.json", opts).unwrap();
    zip.write_all(manifest.to_string().as_bytes()).unwrap();
    for (name, data) in entries {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

fn manifest(assets: Value) -> Value {
    json!({
        "format_version": FORMAT_VERSION,
        "app_version": "test",
        "created_at": "2026-10-06",
        "obs_version": "32.2.2",
        "scene_collections": ["S"],
        "profiles": [],
        "plugins": [],
        "assets": assets,
    })
}

fn asset(n: usize, nom: &str, taille: u64) -> Value {
    json!({
        "original_path": format!("C:/source/{nom}"),
        "archive_path": format!("assets/{n}/{nom}"),
        "file_name": nom,
        "size": taille,
    })
}

/// Noms de tous les fichiers et dossiers sous `root`.
fn noms_sous(root: &Path) -> Vec<String> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

fn aucun_temporaire(sandbox: &Path) {
    let restes: Vec<String> = noms_sous(sandbox)
        .into_iter()
        .filter(|n| n.contains(".tmp-"))
        .collect();
    assert!(restes.is_empty(), "temporaires non nettoyés : {restes:?}");
}

// Un seul test séquentiel : les variables STREAMPOD_* sont globales au processus.
#[test]
fn archives_piegees_neutralisees_ou_refusees() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let sandbox = root.join("machine");
    let config_dst = sandbox.join("obs-studio");
    let assets_dst = sandbox.join("OBS-Backup-Assets");
    fs::create_dir_all(&config_dst).unwrap();
    fs::write(config_dst.join("global.ini"), "[General]\nAvant=oui\n").unwrap();
    std::env::set_var("STREAMPOD_CONFIG_DIR", &config_dst);
    std::env::set_var("STREAMPOD_ASSETS_DIR", &assets_dst);
    std::env::set_var("STREAMPOD_INSTALL_DIR", sandbox.join("obs-install"));
    std::env::set_var("STREAMPOD_OBS_VERSION", "32.2.2");

    // 1. Exécutable, script actif et obs-websocket ouvert sans mot de passe :
    //    la restauration réussit, mais rien de tout cela n'est actif.
    let scene = json!({
        "name": "S",
        "sources": [ { "id": "ffmpeg_source", "settings": { "local_file": "C:/source/outil.exe" } } ],
        "modules": { "scripts-tool": [ { "path": "C:/source/chat.lua", "settings": {} } ] }
    })
    .to_string();
    let piege = root.join("scripts.obsbackup");
    ecrire_archive(
        &piege,
        &manifest(json!([asset(0, "outil.exe", 2), asset(1, "chat.lua", 3)])),
        &[
            ("config/global.ini", b"[General]\nApres=oui\n"),
            ("config/basic/scenes/S.json", scene.as_bytes()),
            (
                "config/plugin_config/obs-websocket/config.json",
                br#"{"server_enabled":true,"auth_required":false,"first_load":false}"#,
            ),
            ("assets/0/outil.exe", b"MZ"),
            ("assets/1/chat.lua", b"lua"),
        ],
    );
    let resultat = restore::restore(&piege, &[], |_| {}, || false).unwrap();
    assert_eq!(
        resultat.assets_restored, 1,
        "l'exécutable n'est pas extrait"
    );
    assert!(!noms_sous(&sandbox).iter().any(|n| n == "outil.exe"));
    let dossier = Path::new(resultat.assets_dir.as_deref().unwrap());
    let script = dossier.join("1").join("chat.lua");
    assert!(script.is_file(), "le fichier du script reste restauré");
    assert_eq!(
        resultat.scripts,
        [script.to_string_lossy().replace('\\', "/")]
    );
    let restauree: Value = serde_json::from_str(
        &fs::read_to_string(config_dst.join("basic").join("scenes").join("S.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(restauree["modules"]["scripts-tool"], json!([]));
    assert_eq!(
        restauree["sources"][0]["settings"]["local_file"], "C:/source/outil.exe",
        "un exécutable n'est jamais référencé vers le dossier d'assets"
    );
    let ws: Value = serde_json::from_str(
        &fs::read_to_string(
            config_dst
                .join("plugin_config")
                .join("obs-websocket")
                .join("config.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(ws["server_enabled"], false);
    assert_eq!(ws["first_load"], true);
    let config_apres = fs::read_to_string(config_dst.join("global.ini")).unwrap();

    // 2. Manifest géant : refusé dès l'aperçu, sans tout charger en mémoire.
    let mut geant = manifest(json!([]));
    geant["remplissage"] = Value::String("A".repeat(17 * 1024 * 1024));
    let piege = root.join("manifest-geant.obsbackup");
    ecrire_archive(&piege, &geant, &[]);
    for e in [
        restore::preview(&piege).map(|_| ()).unwrap_err(),
        restore::restore(&piege, &[], |_| {}, || false)
            .map(|_| ())
            .unwrap_err(),
    ] {
        assert!(e.contains("dépasse la taille autorisée"), "{e}");
    }

    // 3. Entrée plus grosse que sa taille déclarée (bombe de décompression) :
    //    refus, temporaires nettoyés, config et assets intacts.
    let piege = root.join("bombe.obsbackup");
    let gros = vec![0u8; 2 * 1024 * 1024];
    ecrire_archive(
        &piege,
        &manifest(json!([asset(0, "image.png", 4)])),
        &[
            ("config/global.ini", b"[General]\nBombe=oui\n"),
            ("assets/0/image.png", &gros),
        ],
    );
    let e = restore::restore(&piege, &[], |_| {}, || false).unwrap_err();
    assert!(e.contains("dépasse la taille autorisée"), "{e}");
    aucun_temporaire(&sandbox);
    assert_eq!(
        fs::read_to_string(config_dst.join("global.ini")).unwrap(),
        config_apres
    );
    assert!(!noms_sous(&assets_dst).iter().any(|n| n == "image.png"));

    // 4. OBS lancé pendant la restauration : refus juste avant la bascule,
    //    assets retirés, config intacte, aucune copie de sécurité de plus.
    std::thread::sleep(std::time::Duration::from_millis(1100)); // horodatage différent
    let valide = root.join("valide.obsbackup");
    ecrire_archive(
        &valide,
        &manifest(json!([asset(0, "image.png", 4)])),
        &[
            ("config/global.ini", b"[General]\nPendantObs=oui\n"),
            ("assets/0/image.png", b"PNG!"),
        ],
    );
    let copies_avant = noms_sous(&sandbox)
        .iter()
        .filter(|n| n.starts_with("obs-studio.bak-"))
        .count();
    let e = restore::restore_avec_garde(&valide, &[], |_| {}, || false, || true).unwrap_err();
    assert_eq!(e, obs::OBS_RUNNING_MSG);
    aucun_temporaire(&sandbox);
    assert_eq!(
        fs::read_to_string(config_dst.join("global.ini")).unwrap(),
        config_apres
    );
    assert!(!noms_sous(&assets_dst).iter().any(|n| n == "image.png"));
    assert_eq!(
        noms_sous(&sandbox)
            .iter()
            .filter(|n| n.starts_with("obs-studio.bak-"))
            .count(),
        copies_avant
    );

    // 5. OBS lancé pendant la sauvegarde : aucune archive mise en place.
    let sortie = root.join("pendant-obs.obsbackup");
    let e = backup::create_avec_garde(
        &config_dst,
        None,
        None,
        None,
        &sortie,
        |_| {},
        || false,
        || true,
    )
    .unwrap_err();
    assert_eq!(e, obs::OBS_RUNNING_MSG);
    assert!(!sortie.exists());
    assert!(!sortie.with_extension("obsbackup.tmp").exists());
}
