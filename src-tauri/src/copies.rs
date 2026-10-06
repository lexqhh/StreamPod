//! Copies de sécurité `obs-studio.bak-<date>` laissées par les restaurations :
//! liste, retour à une copie (avec rollback) et envoi à la corbeille - jamais
//! de suppression définitive.

use crate::{obs, restore};
use serde::Serialize;
use std::path::{Path, PathBuf};

const PREFIXE: &str = "obs-studio.bak-";

#[derive(Debug, Clone, Serialize)]
pub struct CopieSecurite {
    pub chemin: String,
    pub nom: String,
    /// Date de la copie (`AAAA-MM-JJTHH:MM:SS`), lue dans son nom.
    pub date: Option<String>,
    pub taille: u64,
}

fn dossier_parent(target: &Path) -> Result<&Path, String> {
    target
        .parent()
        .ok_or_else(|| "Dossier de configuration invalide.".to_string())
}

fn cible() -> Result<PathBuf, String> {
    obs::config_dir_target()
        .ok_or_else(|| "Impossible de déterminer le dossier de configuration OBS.".to_string())
}

fn taille_dossier(dossier: &Path) -> u64 {
    walkdir::WalkDir::new(dossier)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Copies voisines de la configuration OBS, de la plus récente à la plus
/// ancienne.
pub fn lister() -> Result<Vec<CopieSecurite>, String> {
    Ok(lister_dans(dossier_parent(&cible()?)?))
}

fn lister_dans(parent: &Path) -> Vec<CopieSecurite> {
    let mut copies: Vec<CopieSecurite> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let nom = e.file_name().to_string_lossy().into_owned();
            let horodatage = nom.strip_prefix(PREFIXE)?;
            let date = horodatage
                .get(..15)
                .and_then(|h| chrono::NaiveDateTime::parse_from_str(h, "%Y%m%d-%H%M%S").ok())
                .map(|d| d.format("%Y-%m-%dT%H:%M:%S").to_string());
            Some(CopieSecurite {
                chemin: e.path().to_string_lossy().into_owned(),
                taille: taille_dossier(&e.path()),
                nom,
                date,
            })
        })
        .collect();
    copies.sort_by(|a, b| b.nom.cmp(&a.nom));
    copies
}

/// Le chemin reçu de l'interface désigne-t-il bien une copie de sécurité
/// voisine de la configuration ? Rien d'autre ne doit pouvoir être basculé
/// ou jeté.
fn valider(chemin: &str, parent: &Path) -> Result<PathBuf, String> {
    let invalide = || "Cette copie de sécurité est introuvable ou invalide.".to_string();
    let copie = PathBuf::from(chemin);
    let nom_ok = copie
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with(PREFIXE));
    let voisine = match (
        copie.parent().map(Path::canonicalize),
        parent.canonicalize(),
    ) {
        (Some(Ok(a)), Ok(b)) => a == b,
        _ => false,
    };
    if nom_ok && voisine && copie.is_dir() {
        Ok(copie)
    } else {
        Err(invalide())
    }
}

/// Remet une copie de sécurité en place ; la configuration actuelle devient
/// à son tour une copie. Retourne le chemin de cette nouvelle copie.
pub fn revenir_a(chemin: &str, obs_ouvert: impl Fn() -> bool) -> Result<Option<String>, String> {
    revenir_a_avec(&cible()?, chemin, obs_ouvert, |a, b| std::fs::rename(a, b))
}

fn revenir_a_avec(
    target: &Path,
    chemin: &str,
    obs_ouvert: impl Fn() -> bool,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<Option<String>, String> {
    let parent = dossier_parent(target)?;
    let copie = valider(chemin, parent)?;
    if obs_ouvert() {
        return Err(obs::OBS_RUNNING_MSG.to_string());
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    // Deux retours dans la même seconde : suffixe pour ne rien écraser.
    let bak = (1..)
        .map(|n| match n {
            1 => parent.join(format!("{PREFIXE}{stamp}")),
            n => parent.join(format!("{PREFIXE}{stamp}-{n}")),
        })
        .find(|p| !p.exists())
        .expect("suite infinie");
    restore::basculer_vers_copie(target, &copie, &bak, rename)
}

/// Envoie une copie de sécurité à la corbeille Windows.
pub fn jeter(chemin: &str) -> Result<(), String> {
    jeter_avec(&cible()?, chemin, corbeille)
}

fn jeter_avec(
    target: &Path,
    chemin: &str,
    corbeille: impl Fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let copie = valider(chemin, dossier_parent(target)?)?;
    corbeille(&copie)
}

/// `SHFileOperationW` avec `FOF_ALLOWUNDO` : corbeille. Si Windows ne peut pas
/// y placer le dossier, `FOF_WANTNUKEWARNING` lui fait demander confirmation
/// au lieu de supprimer définitivement en silence.
#[cfg(windows)]
fn corbeille(dossier: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
        FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW,
    };
    // Liste de chemins terminée par un double zéro.
    let source: Vec<u16> = dossier.as_os_str().encode_wide().chain([0, 0]).collect();
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(source.as_ptr()),
        fFlags: (FOF_ALLOWUNDO
            | FOF_NOCONFIRMATION
            | FOF_SILENT
            | FOF_NOERRORUI
            | FOF_WANTNUKEWARNING)
            .0 as u16,
        ..Default::default()
    };
    let code = unsafe { SHFileOperationW(&mut operation) };
    if code != 0 || operation.fAnyOperationsAborted.as_bool() || dossier.exists() {
        return Err(format!(
            "Impossible de placer « {} » dans la corbeille (code {code}).",
            dossier.display()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn corbeille(_dossier: &Path) -> Result<(), String> {
    Err("Corbeille disponible sous Windows uniquement.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Configuration active, deux copies voisines et un dossier étranger.
    fn bac_a_sable() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("obs-studio");
        for (nom, contenu) in [
            ("obs-studio", "actuelle"),
            ("obs-studio.bak-20260101-120000", "janvier"),
            ("obs-studio.bak-20260301-090000", "mars"),
            ("autre-dossier", "étranger"),
        ] {
            std::fs::create_dir(dir.path().join(nom)).unwrap();
            std::fs::write(dir.path().join(nom).join("global.ini"), contenu).unwrap();
        }
        (dir, target)
    }

    fn lire(dossier: &Path) -> String {
        std::fs::read_to_string(dossier.join("global.ini")).unwrap()
    }

    #[test]
    fn liste_les_copies_de_la_plus_recente_a_la_plus_ancienne() {
        let (dir, _target) = bac_a_sable();
        let copies = lister_dans(dir.path());
        let noms: Vec<&str> = copies.iter().map(|c| c.nom.as_str()).collect();
        assert_eq!(
            noms,
            [
                "obs-studio.bak-20260301-090000",
                "obs-studio.bak-20260101-120000"
            ]
        );
        assert_eq!(copies[0].date.as_deref(), Some("2026-03-01T09:00:00"));
        assert_eq!(copies[0].taille, 4);
    }

    #[test]
    fn retour_a_une_copie_la_config_actuelle_devient_une_copie() {
        let (dir, target) = bac_a_sable();
        let janvier = dir.path().join("obs-studio.bak-20260101-120000");
        let nouvelle = revenir_a_avec(
            &target,
            &janvier.to_string_lossy(),
            || false,
            |a, b| std::fs::rename(a, b),
        )
        .unwrap()
        .expect("la config actuelle est mise de côté");
        assert_eq!(lire(&target), "janvier");
        assert_eq!(lire(Path::new(&nouvelle)), "actuelle");
        assert!(!janvier.exists());
        assert_eq!(lister_dans(dir.path()).len(), 2);
    }

    #[test]
    fn retour_a_une_copie_rollback_sans_perdre_la_copie() {
        let (dir, target) = bac_a_sable();
        let janvier = dir.path().join("obs-studio.bak-20260101-120000");
        let e = revenir_a_avec(
            &target,
            &janvier.to_string_lossy(),
            || false,
            |a, b| {
                if a == janvier {
                    Err(std::io::Error::other("sabotage : verrou simulé"))
                } else {
                    std::fs::rename(a, b)
                }
            },
        )
        .unwrap_err();
        assert!(e.contains("remise en place"), "{e}");
        assert_eq!(lire(&target), "actuelle");
        assert_eq!(lire(&janvier), "janvier", "la copie n'est jamais supprimée");
    }

    #[test]
    fn retour_refuse_si_obs_tourne() {
        let (dir, target) = bac_a_sable();
        let janvier = dir.path().join("obs-studio.bak-20260101-120000");
        let e = revenir_a_avec(
            &target,
            &janvier.to_string_lossy(),
            || true,
            |a, b| std::fs::rename(a, b),
        )
        .unwrap_err();
        assert_eq!(e, obs::OBS_RUNNING_MSG);
        assert_eq!(lire(&target), "actuelle");
    }

    #[test]
    fn seules_les_copies_voisines_sont_acceptees() {
        let (dir, target) = bac_a_sable();
        let ailleurs = tempfile::tempdir().unwrap();
        let copie_ailleurs = ailleurs.path().join("obs-studio.bak-20260101-120000");
        std::fs::create_dir(&copie_ailleurs).unwrap();
        for chemin in [
            dir.path().join("autre-dossier"),
            dir.path().join("obs-studio"),
            dir.path().join("obs-studio.bak-inexistante"),
            copie_ailleurs,
        ] {
            let chemin = chemin.to_string_lossy();
            assert!(
                revenir_a_avec(&target, &chemin, || false, |a, b| std::fs::rename(a, b)).is_err()
            );
            assert!(jeter_avec(&target, &chemin, |_| panic!("jamais appelée")).is_err());
        }
        assert_eq!(lire(&target), "actuelle");
        assert_eq!(lire(&dir.path().join("autre-dossier")), "étranger");
    }

    /// Vraie corbeille Windows : y dépose un petit dossier de test.
    /// Lancé explicitement : cargo test corbeille_reelle -- --ignored
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn corbeille_reelle() {
        let dir = tempfile::tempdir().unwrap();
        let copie = dir.path().join("obs-studio.bak-test-corbeille-streampod");
        std::fs::create_dir(&copie).unwrap();
        std::fs::write(copie.join("global.ini"), "test").unwrap();
        corbeille(&copie).unwrap();
        assert!(!copie.exists());
    }

    #[test]
    fn jeter_passe_par_la_corbeille() {
        let (dir, target) = bac_a_sable();
        let mars = dir.path().join("obs-studio.bak-20260301-090000");
        let appels = std::cell::RefCell::new(Vec::new());
        jeter_avec(&target, &mars.to_string_lossy(), |p| {
            appels.borrow_mut().push(p.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(*appels.borrow(), [mars]);
    }
}
