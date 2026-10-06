//! Remplacement de l'exécutable portable sur des chemins injectés : jamais
//! d'état sans exécutable, signature vérifiée avant toute écriture.

use std::fs;
use std::io;
use std::path::Path;
use streampod_lib::maj;

/// Clé de test jetable (sans lien avec la clé de release) et signature de
/// `NOUVEAU`, produites par `tauri signer`.
const CLE_TEST: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IERGNzkxNDJEQ0QxMDI2N0IKUldSN0poRE5MUlI1M3g3WWtLTkR3bW54T1VERDlETkU4OHptSFRIaUxJaFBnaGpBaFZBOUpNbGcK";
const SIG_TEST: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSN0poRE5MUlI1MzVWUXF6WlBmS01CWk5IQ1R3VUdGVFM5TDZ5WndZRGR0VHdqSXBORUhOZ2NteEFrQjBhb0tvYVh4N3BubDd0RkFkaC9TLzF3VUJZMCtiV1lVRU1RMGdVPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxMzE0MDQ2CWZpbGU6bm91dmVhdS5leGUKUXVwdFVoSjQ3dkN3bG40S05UUjBGU1Vtd2tKZ0g2eUgvNE1xRXNQQzNtTm5CWVlRZ3F6Y2ZxTFZkUmtxa3F2dGxpRU9PRUs0dnRIUUlBL0lkS20vQ1E9PQo=";
const NOUVEAU: &[u8] = b"nouvelle version factice";
const ACTUEL: &[u8] = b"version actuelle";

fn bac_a_sable() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    // Nom personnalisé : il doit être conservé.
    let exe = dir.path().join("Mon StreamPod.exe");
    fs::write(&exe, ACTUEL).unwrap();
    (dir, exe)
}

fn fichiers(dir: &Path) -> Vec<String> {
    let mut noms: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    noms.sort();
    noms
}

#[test]
fn remplacement_nominal() {
    let (dir, exe) = bac_a_sable();
    maj::installer_portable(&exe, NOUVEAU, SIG_TEST, CLE_TEST).unwrap();
    assert_eq!(fs::read(&exe).unwrap(), NOUVEAU);
    assert_eq!(fs::read(maj::chemin_ancien(&exe)).unwrap(), ACTUEL);
    assert_eq!(
        fichiers(dir.path()),
        ["Mon StreamPod.exe", "Mon StreamPod.exe.old"]
    );
}

#[test]
fn echec_du_second_renommage_remet_l_exe_en_place() {
    let (dir, exe) = bac_a_sable();
    let mut appels = 0;
    let err = maj::remplacer_exe(&exe, NOUVEAU, |de, vers| {
        appels += 1;
        if appels == 2 {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "antivirus"))
        } else {
            fs::rename(de, vers)
        }
    })
    .unwrap_err();
    assert!(err.contains("mise en place"), "{err}");
    assert_eq!(fs::read(&exe).unwrap(), ACTUEL);
    assert_eq!(fichiers(dir.path()), ["Mon StreamPod.exe"]);
}

#[test]
fn double_echec_garde_l_exe_d_origine_et_l_explique() {
    let (dir, exe) = bac_a_sable();
    let mut appels = 0;
    let err = maj::remplacer_exe(&exe, NOUVEAU, |de, vers| {
        appels += 1;
        if appels >= 2 {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "verrouillé",
            ))
        } else {
            fs::rename(de, vers)
        }
    })
    .unwrap_err();
    let ancien = maj::chemin_ancien(&exe);
    assert_eq!(fs::read(&ancien).unwrap(), ACTUEL);
    assert!(err.contains(&ancien.display().to_string()), "{err}");
    assert_eq!(fichiers(dir.path()), ["Mon StreamPod.exe.old"]);
}

#[test]
fn echec_du_premier_renommage_ne_touche_a_rien() {
    let (dir, exe) = bac_a_sable();
    let err = maj::remplacer_exe(&exe, NOUVEAU, |_, _| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "verrouillé",
        ))
    })
    .unwrap_err();
    assert!(err.contains("version actuelle"), "{err}");
    assert_eq!(fichiers(dir.path()), ["Mon StreamPod.exe"]);
}

#[test]
fn signature_invalide_refusee_sans_toucher_aux_fichiers() {
    let (dir, exe) = bac_a_sable();
    let err = maj::installer_portable(&exe, b"exe altere", SIG_TEST, CLE_TEST).unwrap_err();
    assert!(err.contains("Signature"), "{err}");
    assert!(maj::installer_portable(&exe, NOUVEAU, "pas une signature", CLE_TEST).is_err());
    assert_eq!(fs::read(&exe).unwrap(), ACTUEL);
    assert_eq!(fichiers(dir.path()), ["Mon StreamPod.exe"]);
}

/// Refus d'écriture posé par ACL : l'attribut lecture seule d'un dossier est
/// ignoré par Windows pour la création de fichiers.
#[cfg(windows)]
#[test]
fn dossier_en_lecture_seule_refuse_sans_modification() {
    use std::process::Command;
    let (dir, exe) = bac_a_sable();
    let chemin = dir.path().to_string_lossy().into_owned();
    let icacls = |args: &[&str]| {
        assert!(Command::new("icacls")
            .arg(&chemin)
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    };
    icacls(&["/deny", "*S-1-1-0:(W)"]);
    let resultat = maj::installer_portable(&exe, NOUVEAU, SIG_TEST, CLE_TEST);
    icacls(&["/remove:d", "*S-1-1-0"]);
    assert_eq!(resultat.unwrap_err(), maj::MSG_LECTURE_SEULE);
    assert_eq!(fs::read(&exe).unwrap(), ACTUEL);
    assert_eq!(fichiers(dir.path()), ["Mon StreamPod.exe"]);
}

#[test]
fn nettoyage_du_old_au_demarrage() {
    let (dir, exe) = bac_a_sable();
    fs::write(maj::chemin_ancien(&exe), ACTUEL).unwrap();
    maj::nettoyer_ancien(&exe, false);
    assert_eq!(fichiers(dir.path()), ["Mon StreamPod.exe"]);
    // Sans .old : rien à faire, aucune erreur.
    maj::nettoyer_ancien(&exe, true);
}
