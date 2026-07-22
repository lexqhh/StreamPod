//! Test adversarial : une archive .obsbackup piégée (zip slip) doit être
//! refusée en bloc, sans écrire le moindre fichier hors du bac à sable.

use streampod_lib::backup::{AssetEntry, Manifest, PluginInfo, FORMAT_VERSION};
use streampod_lib::restore;
use std::fs;
use std::io::Write;
use std::path::Path;

fn manifest_minimal() -> Manifest {
    Manifest {
        format_version: FORMAT_VERSION,
        app_version: "test".to_string(),
        created_at: "2026-07-18".to_string(),
        obs_version: Some("31.0.2".to_string()),
        scene_collections: vec![],
        profiles: vec![],
        plugins: vec![],
        assets: vec![],
    }
}

/// Écrit une archive contenant le manifest donné plus des entrées arbitraires
/// (noms non validés : c'est le but du test).
fn write_archive(path: &Path, manifest: &Manifest, entries: &[(&str, &[u8])]) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    zip.start_file("manifest.json", opts).unwrap();
    zip.write_all(serde_json::to_string(manifest).unwrap().as_bytes())
        .unwrap();
    for (name, data) in entries {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

fn restauration_refusee(backup: &Path) {
    let e = restore::restore(backup, &[], |_| {}, || false)
        .expect_err("la restauration aurait dû être refusée");
    assert!(
        e.contains("Archive invalide ou malveillante"),
        "message inattendu : {e}"
    );
}

// Un seul test séquentiel : les variables d'environnement STREAMPOD_* sont
// globales au processus, des scénarios en parallèle se marcheraient dessus.
#[test]
fn archive_piegee_refusee_sans_ecriture_hors_bac_a_sable() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Bac à sable : une config OBS existante avec un fichier sentinelle.
    let config_dst = root.join("obs-studio");
    fs::create_dir_all(&config_dst).unwrap();
    let sentinelle = "[General]\nSentinelle=oui\n";
    fs::write(config_dst.join("global.ini"), sentinelle).unwrap();
    let assets_dst = root.join("OBS-Backup-Assets");
    std::env::set_var("STREAMPOD_CONFIG_DIR", &config_dst);
    std::env::set_var("STREAMPOD_ASSETS_DIR", &assets_dst);
    std::env::set_var("STREAMPOD_INSTALL_DIR", root.join("obs-install"));
    std::env::set_var("STREAMPOD_OBS_VERSION", "31.0.2");

    // Cibles sentinelles DANS le tempdir (jamais un vrai chemin système) :
    // si la faille réapparaît, l'écriture atterrit ici, hors des dossiers
    // de restauration légitimes.
    let evil1 = root.join("evil.dll");
    let evil2 = root.join("evil2.txt");
    let evil3 = root.join("evil3.txt");

    // Scénario 1 (PoC) : entrée config/ avec chemin absolu Windows —
    // PathBuf::join abandonnerait la base.
    let piege1 = root.join("piege1.obsbackup");
    write_archive(
        &piege1,
        &manifest_minimal(),
        &[(&format!("config/{}", evil1.display()), b"MECHANT")],
    );
    restauration_refusee(&piege1);

    // Scénario 2 : traversée par ..\ dans une entrée config/.
    let piege2 = root.join("piege2.obsbackup");
    write_archive(
        &piege2,
        &manifest_minimal(),
        &[(r"config/..\..\evil2.txt", b"MECHANT")],
    );
    restauration_refusee(&piege2);

    // Scénario 3 : archive_path piégé dans le manifest (vecteur assets).
    let mut manifest3 = manifest_minimal();
    let archive_path = format!("assets/{}", evil3.display());
    manifest3.assets.push(AssetEntry {
        original_path: "C:/source/evil3.txt".to_string(),
        archive_path: archive_path.clone(),
        file_name: "evil3.txt".to_string(),
        size: 7,
    });
    let piege3 = root.join("piege3.obsbackup");
    write_archive(&piege3, &manifest3, &[(archive_path.as_str(), b"MECHANT")]);
    restauration_refusee(&piege3);

    // Aucun fichier n'a été écrit hors du bac à sable.
    assert!(!evil1.exists(), "le PoC zip slip a écrit {}", evil1.display());
    assert!(!evil2.exists(), "la traversée ..\\ a écrit {}", evil2.display());
    assert!(!evil3.exists(), "le manifest piégé a écrit {}", evil3.display());
    assert!(!assets_dst.exists(), "aucun asset ne devait être restauré");

    // La configuration d'origine est intacte : contenu inchangé, pas de
    // copie de sécurité créée (l'échec survient avant la bascule).
    assert_eq!(
        fs::read_to_string(config_dst.join("global.ini")).unwrap(),
        sentinelle
    );
    let bak_cree = fs::read_dir(root)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("obs-studio.bak-"));
    assert!(!bak_cree, "aucune copie de sécurité ne doit être créée sur refus");

    // Scénario 4 : DLL arbitraire. L'archive embarque une DLL sous
    // plugins/64bit/ visant à écraser un plugin officiel, et son manifest —
    // écrit par l'auteur de l'archive — revendique la même version majeure
    // d'OBS que la machine. La restauration réussit, mais aucune DLL ne doit
    // être écrite : les plugins sont seulement listés (statut "manual").
    let install_dir = root.join("obs-install");
    let dll_officielle = install_dir
        .join("obs-plugins")
        .join("64bit")
        .join("obs-websocket.dll");
    fs::create_dir_all(dll_officielle.parent().unwrap()).unwrap();
    fs::write(&dll_officielle, "DLL OFFICIELLE").unwrap();

    let mut manifest4 = manifest_minimal(); // obs_version 31.0.2 = celle du "PC"
    manifest4.plugins.push(PluginInfo {
        name: "obs-websocket".to_string(),
        dll: "obs-websocket.dll".to_string(),
        size: 7,
        has_data_dir: true,
    });
    let piege4 = root.join("piege4.obsbackup");
    write_archive(
        &piege4,
        &manifest4,
        &[
            ("config/global.ini", b"[General]\n"),
            ("plugins/64bit/obs-websocket.dll", b"MECHANTE-DLL"),
            ("plugins/data/obs-websocket/evil.lua", b"MECHANT"),
        ],
    );
    let resultat = restore::restore(&piege4, &[], |_| {}, || false)
        .expect("la restauration de la config elle-même doit réussir");
    assert_eq!(resultat.plugins_status, "manual");
    assert_eq!(resultat.plugins, vec!["obs-websocket".to_string()]);
    assert_eq!(
        fs::read_to_string(&dll_officielle).unwrap(),
        "DLL OFFICIELLE",
        "une DLL de l'installation OBS a été écrasée par l'archive"
    );
    assert!(
        !install_dir
            .join("data")
            .join("obs-plugins")
            .join("obs-websocket")
            .exists(),
        "des fichiers de plugin ont été écrits depuis l'archive"
    );
}
