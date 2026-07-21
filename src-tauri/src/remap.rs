//! Diagnostic en lecture seule du remappage matériel (étape 2 du plan).
//!
//! Lit les collections de scènes directement dans l'archive `.obsbackup`
//! — sans extraction, sans dossier temporaire, sans copie d'asset — puis
//! compare les périphériques référencés avec l'inventaire de ce PC pour
//! distinguer les références encore valides des associations à confirmer.
//!
//! Seuls les trois types de sources natives du MVP sont reconnus (liste
//! blanche) ; toute autre source reste strictement ignorée. L'audio global
//! (`Mic/Aux`, audio du bureau) utilise le même schéma natif et vit aux clés
//! racine `AuxAudioDevice*` / `DesktopAudioDevice*` des collections.

use crate::devices::{self, Famille, Peripherique};
use crate::restore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

/// Taille maximale d'un JSON de collection lu depuis l'archive. Les
/// collections réelles font quelques centaines de Ko ; au-delà de cette
/// limite, le contenu est anormal et l'archive est refusée.
const TAILLE_MAX_COLLECTION: u64 = 32 * 1024 * 1024;

fn err(context: &str, e: impl std::fmt::Display) -> String {
    format!("{context} : {e}")
}

/// Une source qui référence un périphérique donné.
#[derive(Debug, Clone, Serialize)]
pub struct SourceConcernee {
    pub collection: String,
    pub source: String,
}

/// Un ancien identifiant matériel absent de ce PC, regroupé sur toutes ses
/// occurrences : une seule décision de l'utilisateur suffira.
#[derive(Debug, Clone, Serialize)]
pub struct Association {
    pub famille: Famille,
    /// Identifiant tel qu'écrit par OBS dans la sauvegarde.
    pub ancien_id: String,
    /// Nom convivial de l'ancien périphérique : extrait de l'identifiant pour
    /// la vidéo, sinon nom de la première source qui l'utilise.
    pub ancien_nom: String,
    pub sources: Vec<SourceConcernee>,
    pub occurrences: usize,
    /// Périphériques de ce PC compatibles avec la famille, du plus
    /// ressemblant au moins ressemblant. Sert uniquement à ordonner la liste
    /// proposée : jamais de sélection automatique.
    pub candidats: Vec<Peripherique>,
}

/// Résultat du diagnostic, sans aucune modification sur le disque.
#[derive(Debug, Clone, Serialize)]
pub struct RemapReport {
    /// Périphériques absents de ce PC : à confirmer par l'utilisateur.
    pub a_confirmer: Vec<Association>,
    /// Nombre de références encore valides (`default` inclus), conservées
    /// sans question.
    pub references_valides: usize,
    /// Inventaire des périphériques actifs de ce PC (candidats de
    /// remplacement, à filtrer par famille).
    pub peripheriques: Vec<Peripherique>,
}

/// Liste blanche du MVP : type de source OBS → famille + champ matériel
/// documenté. Toute autre source est ignorée par l'analyse et la réécriture.
fn champ_materiel(type_source: &str) -> Option<(Famille, &'static str)> {
    match type_source {
        "wasapi_input_capture" => Some((Famille::EntreeAudio, "device_id")),
        "wasapi_output_capture" => Some((Famille::SortieAudio, "device_id")),
        "dshow_input" => Some((Famille::Video, "video_device_id")),
        _ => None,
    }
}

/// Champ matériel documenté d'une source reconnue du MVP.
/// Retourne `None` pour toute source hors liste blanche.
fn reference_materielle(source: &Value) -> Option<(Famille, &str)> {
    let (famille, champ) = champ_materiel(source.get("id")?.as_str()?)?;
    let id = source.get("settings")?.get(champ)?.as_str()?;
    if id.is_empty() {
        return None;
    }
    Some((famille, id))
}

/// Clé de regroupement : famille + identifiant normalisé (les identifiants
/// OBS et Windows ne diffèrent parfois que par la casse).
type CleAssociation = (Famille, String);

fn normaliser_id(id: &str) -> String {
    id.to_lowercase()
}

/// Normalise un nom de périphérique pour la comparaison : minuscules
/// Unicode, translittération légère des caractères de marque courants,
/// ponctuation et espaces réduits. Les noms affichés ou écrits ne sont
/// jamais modifiés — cette forme ne sert qu'au tri des candidats.
fn normaliser_nom(nom: &str) -> String {
    let mut s = String::new();
    for c in nom.to_lowercase().chars() {
        match c {
            'ø' => s.push('o'),
            'æ' => s.push_str("ae"),
            'œ' => s.push_str("oe"),
            'ß' => s.push_str("ss"),
            'à' | 'â' | 'ä' | 'á' => s.push('a'),
            'é' | 'è' | 'ê' | 'ë' => s.push('e'),
            'î' | 'ï' | 'í' => s.push('i'),
            'ô' | 'ö' | 'ó' => s.push('o'),
            'û' | 'ù' | 'ü' | 'ú' => s.push('u'),
            'ç' => s.push('c'),
            c if c.is_alphanumeric() => s.push(c),
            _ => s.push(' '),
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tokens_communs(a: &str, b: &str) -> usize {
    a.split(' ')
        .filter(|t| !t.is_empty() && b.split(' ').any(|u| u == *t))
        .count()
}

/// Ordonne les périphériques compatibles avec la famille, du plus
/// ressemblant au moins ressemblant : nom normalisé identique, puis
/// inclusion d'un nom dans l'autre, puis nombre de mots communs. À égalité,
/// l'ordre alphabétique garantit un tri déterministe.
fn candidats(
    famille: Famille,
    ancien_nom: &str,
    inventaire: &[Peripherique],
) -> Vec<Peripherique> {
    let ancien = normaliser_nom(ancien_nom);
    let mut compatibles: Vec<&Peripherique> =
        inventaire.iter().filter(|p| p.famille == famille).collect();
    compatibles.sort_by_cached_key(|p| {
        let cand = normaliser_nom(&p.nom);
        let rang = if !ancien.is_empty() && cand == ancien {
            0u8
        } else if !ancien.is_empty()
            && !cand.is_empty()
            && (cand.contains(&ancien) || ancien.contains(&cand))
        {
            1
        } else {
            2
        };
        (rang, Reverse(tokens_communs(&ancien, &cand)), cand, p.id.clone())
    });
    compatibles.into_iter().cloned().collect()
}

/// Recense les références matérielles d'une collection : sources de scènes
/// (`sources[]`) et audio global (clés racine `AuxAudioDevice*` /
/// `DesktopAudioDevice*`), qui partagent le même schéma natif.
fn analyser_collection(
    nom_collection: &str,
    racine: &Value,
    references: &mut BTreeMap<CleAssociation, Association>,
) {
    let Some(objet) = racine.as_object() else {
        return;
    };

    let sources_scenes = objet
        .get("sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    let audio_global = objet
        .iter()
        .filter(|(cle, _)| {
            cle.starts_with("AuxAudioDevice") || cle.starts_with("DesktopAudioDevice")
        })
        .map(|(_, v)| v);

    for source in sources_scenes.chain(audio_global) {
        let Some((famille, id)) = reference_materielle(source) else {
            continue;
        };
        let nom_source = source
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Source sans nom")
            .to_string();
        let entree = references
            .entry((famille, normaliser_id(id)))
            .or_insert_with(|| Association {
                famille,
                ancien_id: id.to_string(),
                ancien_nom: match famille {
                    Famille::Video => {
                        devices::nom_video(id).unwrap_or_else(|| nom_source.clone())
                    }
                    _ => nom_source.clone(),
                },
                sources: Vec::new(),
                occurrences: 0,
                candidats: Vec::new(),
            });
        entree.occurrences += 1;
        entree.sources.push(SourceConcernee {
            collection: nom_collection.to_string(),
            source: nom_source,
        });
    }
}

/// Une référence est valide si le périphérique existe encore sur ce PC —
/// ou s'il s'agit de la valeur audio spéciale `default` (« périphérique par
/// défaut du système »), qui ne correspond à aucun matériel précis.
fn est_valide(famille: Famille, id_normalise: &str, inventaire: &[Peripherique]) -> bool {
    if famille != Famille::Video && id_normalise == devices::AUDIO_PAR_DEFAUT {
        return true;
    }
    inventaire
        .iter()
        .any(|p| p.famille == famille && normaliser_id(&p.id) == id_normalise)
}

/// Analyse une archive `.obsbackup` en lecture seule et rapporte les
/// associations à confirmer. Ne crée ni dossier temporaire ni fichier :
/// l'annulation après ce diagnostic ne demande aucun nettoyage.
pub fn analyser(
    backup_path: &Path,
    peripheriques: Vec<Peripherique>,
) -> Result<RemapReport, String> {
    let mut archive = restore::open_archive(backup_path)?;

    let mut references: BTreeMap<CleAssociation, Association> = BTreeMap::new();
    for i in 0..archive.len() {
        let (nom_entree, taille, est_dossier) = {
            let entree = archive
                .by_index(i)
                .map_err(|e| err("Lecture de l'archive", e))?;
            (entree.name().to_string(), entree.size(), entree.is_dir())
        };
        let Some(rel) = nom_entree.strip_prefix("config/basic/scenes/") else {
            continue;
        };
        if est_dossier || !rel.to_lowercase().ends_with(".json") {
            continue;
        }
        // Même validation que la restauration : un chemin suspect sous un
        // préfixe connu invalide toute l'archive.
        restore::chemin_relatif_sur(Path::new("scenes"), rel)?;
        if taille > TAILLE_MAX_COLLECTION {
            return Err(format!(
                "Archive invalide : la collection « {rel} » est anormalement volumineuse."
            ));
        }

        let mut texte = String::new();
        archive
            .by_index(i)
            .map_err(|e| err("Lecture de l'archive", e))?
            .read_to_string(&mut texte)
            .map_err(|e| err(&format!("Lecture de la collection {rel}"), e))?;
        // Un JSON illisible n'est pas remappable : il est ignoré ici, comme
        // il l'est par la réécriture des chemins d'assets à la restauration.
        let Ok(racine) = serde_json::from_str::<Value>(&texte) else {
            continue;
        };
        let nom_collection = racine
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_else(|| rel.trim_end_matches(".json"))
            .to_string();
        analyser_collection(&nom_collection, &racine, &mut references);
    }

    let mut a_confirmer = Vec::new();
    let mut references_valides = 0usize;
    for ((famille, id_normalise), mut association) in references {
        if est_valide(famille, &id_normalise, &peripheriques) {
            references_valides += association.occurrences;
        } else {
            association.candidats =
                candidats(famille, &association.ancien_nom, &peripheriques);
            a_confirmer.push(association);
        }
    }

    Ok(RemapReport {
        a_confirmer,
        references_valides,
        peripheriques,
    })
}

/// Choix explicite de l'utilisateur : remplacer partout `ancien_id` par
/// `nouveau_id` au sein d'une famille. « Laisser inchangé » ne produit
/// simplement aucun choix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choix {
    pub famille: Famille,
    pub ancien_id: String,
    pub nouveau_id: String,
}

/// Revalide les choix reçus de l'interface contre l'inventaire actuel de ce
/// PC et les associations manquantes retrouvées dans l'archive. Refuse un
/// doublon, une ancienne référence qui n'est pas à confirmer, une famille
/// incompatible ou un périphérique devenu absent.
///
/// Les identifiants retournés sont ceux de l'inventaire Rust : une variante
/// de casse acceptée à la comparaison n'est jamais recopiée depuis
/// l'interface dans la configuration OBS.
pub fn valider_choix(
    choix: &[Choix],
    inventaire: &[Peripherique],
    associations: &[Association],
) -> Result<Vec<Choix>, String> {
    let mut vus = std::collections::BTreeSet::new();
    let autorises: BTreeMap<CleAssociation, &Association> = associations
        .iter()
        .map(|a| ((a.famille, normaliser_id(&a.ancien_id)), a))
        .collect();
    let mut valides = Vec::with_capacity(choix.len());
    for c in choix {
        let cle = (c.famille, normaliser_id(&c.ancien_id));
        if !vus.insert(cle.clone()) {
            return Err(format!(
                "Choix de remappage invalides : deux remplacements différents pour \
                 « {} ». Relancez la restauration.",
                c.ancien_id
            ));
        }
        let Some(association) = autorises.get(&cle) else {
            return Err(format!(
                "Choix de remappage invalide : « {} » ne fait pas partie des \
                 périphériques manquants de cette sauvegarde. Relancez la restauration.",
                c.ancien_id
            ));
        };
        let disponible = inventaire.iter().find(|p| {
            p.famille == c.famille && normaliser_id(&p.id) == normaliser_id(&c.nouveau_id)
        });
        let Some(disponible) = disponible else {
            return Err(format!(
                "Le périphérique choisi en remplacement de « {} » n'est plus disponible \
                 sur ce PC (débranché ou incompatible). Relancez la restauration pour \
                 refaire ce choix.",
                c.ancien_id
            ));
        };
        valides.push(Choix {
            famille: c.famille,
            ancien_id: association.ancien_id.clone(),
            nouveau_id: disponible.id.clone(),
        });
    }
    Ok(valides)
}

/// Remplace le champ matériel d'une source reconnue si son identifiant fait
/// l'objet d'un choix. Retourne la clé du choix appliqué, `None` sinon.
/// Tous les autres champs de la source restent inchangés.
fn remapper_source(
    source: &mut Value,
    nouveaux: &BTreeMap<CleAssociation, String>,
) -> Option<CleAssociation> {
    let (famille, id) = reference_materielle(source)?;
    let champ = champ_materiel(source.get("id")?.as_str()?)?.1;
    let cle = (famille, normaliser_id(id));
    let remplacant = nouveaux.get(&cle)?.clone();
    let settings = source.get_mut("settings")?.as_object_mut()?;
    settings.insert(champ.to_string(), Value::String(remplacant.clone()));
    // OBS maintient `last_video_device_id` en miroir de `video_device_id`
    // (docs/FORMATS-OBS.md) : les deux champs sont réécrits à l'identique.
    if famille == Famille::Video && settings.contains_key("last_video_device_id") {
        settings.insert("last_video_device_id".to_string(), Value::String(remplacant));
    }
    Some(cle)
}

/// Applique les remplacements dans une collection : mêmes emplacements que
/// l'analyse (`sources[]` + audio global aux clés racine). Retourne le nombre
/// de sources modifiées et incrémente le compte par choix.
fn remapper_collection(
    racine: &mut Value,
    nouveaux: &BTreeMap<CleAssociation, String>,
    compte: &mut BTreeMap<CleAssociation, usize>,
) -> usize {
    let Some(objet) = racine.as_object_mut() else {
        return 0;
    };
    let mut modifiees = 0;
    for (cle, valeur) in objet.iter_mut() {
        if cle == "sources" {
            let Some(sources) = valeur.as_array_mut() else { continue };
            for source in sources {
                if let Some(k) = remapper_source(source, nouveaux) {
                    *compte.entry(k).or_default() += 1;
                    modifiees += 1;
                }
            }
        } else if cle.starts_with("AuxAudioDevice") || cle.starts_with("DesktopAudioDevice") {
            if let Some(k) = remapper_source(valeur, nouveaux) {
                *compte.entry(k).or_default() += 1;
                modifiees += 1;
            }
        }
    }
    modifiees
}

/// Applique les choix validés dans les collections de scènes extraites
/// (`obs-studio.tmp-*/basic/scenes`), avant la bascule. Seul le champ
/// matériel documenté des sources reconnues est réécrit ; toute source
/// inconnue reste strictement intacte.
///
/// Vérifie ensuite que chaque choix a remplacé au moins une occurrence :
/// sinon, la sauvegarde ne correspond plus à l'aperçu et la restauration
/// doit être arrêtée avant la bascule.
///
/// Retourne le nombre total de sources modifiées.
pub fn appliquer(scenes_dir: &Path, choix: &[Choix]) -> Result<usize, String> {
    if choix.is_empty() {
        return Ok(0);
    }
    let nouveaux: BTreeMap<CleAssociation, String> = choix
        .iter()
        .map(|c| ((c.famille, normaliser_id(&c.ancien_id)), c.nouveau_id.clone()))
        .collect();
    let mut compte: BTreeMap<CleAssociation, usize> = BTreeMap::new();

    let mut sources_remappees = 0;
    if scenes_dir.is_dir() {
        for entry in std::fs::read_dir(scenes_dir)
            .map_err(|e| err("Lecture des scènes restaurées", e))?
            .flatten()
        {
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                continue;
            }
            let texte = std::fs::read_to_string(&path)
                .map_err(|e| err(&format!("Lecture de {}", path.display()), e))?;
            // Un JSON illisible n'a pas été analysé non plus : il reste tel quel.
            let Ok(mut racine) = serde_json::from_str::<Value>(&texte) else {
                continue;
            };
            let modifiees = remapper_collection(&mut racine, &nouveaux, &mut compte);
            if modifiees > 0 {
                std::fs::write(&path, serde_json::to_string_pretty(&racine).unwrap())
                    .map_err(|e| err(&format!("Écriture de {}", path.display()), e))?;
                sources_remappees += modifiees;
            }
        }
    }

    for c in choix {
        let applique = compte
            .get(&(c.famille, normaliser_id(&c.ancien_id)))
            .copied()
            .unwrap_or(0);
        if applique == 0 {
            return Err(format!(
                "Le remplacement de « {} » n'a trouvé aucune source correspondante dans la \
                 sauvegarde : elle a peut-être changé depuis l'aperçu. Relancez la \
                 restauration.",
                c.ancien_id
            ));
        }
    }
    Ok(sources_remappees)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn peripherique(famille: Famille, id: &str, nom: &str) -> Peripherique {
        Peripherique {
            famille,
            id: id.to_string(),
            nom: nom.to_string(),
        }
    }

    /// Écrit une archive .obsbackup factice contenant les collections
    /// fournies (nom de fichier → contenu).
    fn archive_factice(dir: &Path, collections: &[(&str, String)]) -> std::path::PathBuf {
        let path = dir.join("test.obsbackup");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(br#"{"format_version":1,"app_version":"test","created_at":"","obs_version":null,"scene_collections":[],"profiles":[],"plugins":[],"assets":[]}"#).unwrap();
        for (nom, contenu) in collections {
            zip.start_file(format!("config/basic/scenes/{nom}"), options)
                .unwrap();
            zip.write_all(contenu.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn collection_type(nom: &str, sources: Value) -> String {
        json!({ "name": nom, "sources": sources }).to_string()
    }

    #[test]
    fn reconnait_les_trois_familles_du_mvp() {
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[(
                "Stream.json",
                collection_type(
                    "Stream",
                    json!([
                        { "id": "wasapi_input_capture", "name": "Micro",
                          "settings": { "device_id": "{0.0.1.00000000}.{aaaa}" } },
                        { "id": "wasapi_output_capture", "name": "Musique",
                          "settings": { "device_id": "{0.0.0.00000000}.{bbbb}" } },
                        { "id": "dshow_input", "name": "Webcam",
                          "settings": { "video_device_id": "Webcam C900:chemin#22usb" } }
                    ]),
                ),
            )],
        );

        let rapport = analyser(&archive, vec![]).unwrap();
        assert_eq!(rapport.a_confirmer.len(), 3);
        let familles: Vec<Famille> =
            rapport.a_confirmer.iter().map(|a| a.famille).collect();
        assert!(familles.contains(&Famille::EntreeAudio));
        assert!(familles.contains(&Famille::SortieAudio));
        assert!(familles.contains(&Famille::Video));
        // Le nom convivial vidéo vient de l'identifiant, pas de la source.
        let video = rapport
            .a_confirmer
            .iter()
            .find(|a| a.famille == Famille::Video)
            .unwrap();
        assert_eq!(video.ancien_nom, "Webcam C900");
        assert_eq!(video.ancien_id, "Webcam C900:chemin#22usb");
    }

    #[test]
    fn ignore_une_source_inconnue_avec_une_cle_similaire() {
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[(
                "Stream.json",
                collection_type(
                    "Stream",
                    json!([
                        // Source d'un plugin tiers avec un champ au nom
                        // ressemblant : hors liste blanche, jamais analysée.
                        { "id": "plugin_tiers_capture", "name": "Deck",
                          "settings": { "device_id": "{0.0.1.00000000}.{cccc}" } },
                        // Type connu mais champ matériel absent : rien à faire.
                        { "id": "wasapi_input_capture", "name": "Micro sans device",
                          "settings": { "use_device_timing": false } }
                    ]),
                ),
            )],
        );

        let rapport = analyser(&archive, vec![]).unwrap();
        assert!(rapport.a_confirmer.is_empty());
        assert_eq!(rapport.references_valides, 0);
    }

    #[test]
    fn regroupe_les_occurrences_du_meme_identifiant() {
        let micro = json!({ "id": "wasapi_input_capture", "name": "Micro principal",
                            "settings": { "device_id": "{0.0.1.00000000}.{AAAA}" } });
        let micro_bis = json!({ "id": "wasapi_input_capture", "name": "Micro cam",
                                "settings": { "device_id": "{0.0.1.00000000}.{aaaa}" } });
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[
                ("Stream.json", collection_type("Stream", json!([micro]))),
                ("Record.json", collection_type("Record", json!([micro_bis]))),
            ],
        );

        let rapport = analyser(&archive, vec![]).unwrap();
        // Une seule question malgré deux occurrences (casse différente).
        assert_eq!(rapport.a_confirmer.len(), 1);
        let assoc = &rapport.a_confirmer[0];
        assert_eq!(assoc.occurrences, 2);
        assert_eq!(assoc.sources.len(), 2);
        let collections: Vec<&str> =
            assoc.sources.iter().map(|s| s.collection.as_str()).collect();
        assert!(collections.contains(&"Stream"));
        assert!(collections.contains(&"Record"));
    }

    #[test]
    fn default_reste_une_reference_audio_valide() {
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[(
                "Stream.json",
                json!({
                    "name": "Stream",
                    "AuxAudioDevice1": { "id": "wasapi_input_capture", "name": "Mic/Aux",
                                         "settings": { "device_id": "default" } },
                    "DesktopAudioDevice1": { "id": "wasapi_output_capture", "name": "Audio du bureau",
                                             "settings": { "device_id": "default" } },
                    "sources": []
                })
                .to_string(),
            )],
        );

        // Inventaire vide : `default` doit rester valide malgré tout.
        let rapport = analyser(&archive, vec![]).unwrap();
        assert!(rapport.a_confirmer.is_empty());
        assert_eq!(rapport.references_valides, 2);
    }

    #[test]
    fn default_nest_pas_une_valeur_speciale_pour_la_video() {
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[(
                "S.json",
                collection_type(
                    "S",
                    json!([{ "id": "dshow_input", "name": "Cam",
                             "settings": { "video_device_id": "default" } }]),
                ),
            )],
        );

        let rapport = analyser(&archive, vec![]).unwrap();
        assert_eq!(rapport.a_confirmer.len(), 1);
    }

    #[test]
    fn audio_global_detecte_comme_les_sources_de_scenes() {
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[(
                "Stream.json",
                json!({
                    "name": "Stream",
                    "AuxAudioDevice2": { "id": "wasapi_input_capture", "name": "Micro",
                                         "settings": { "device_id": "{0.0.1.00000000}.{dddd}" } },
                    "sources": []
                })
                .to_string(),
            )],
        );

        let rapport = analyser(&archive, vec![]).unwrap();
        assert_eq!(rapport.a_confirmer.len(), 1);
        assert_eq!(rapport.a_confirmer[0].famille, Famille::EntreeAudio);
        assert_eq!(rapport.a_confirmer[0].ancien_nom, "Micro");
    }

    #[test]
    fn peripherique_encore_valide_aucune_question() {
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[(
                "Stream.json",
                collection_type(
                    "Stream",
                    json!([{ "id": "wasapi_input_capture", "name": "Micro",
                             "settings": { "device_id": "{0.0.1.00000000}.{AAAA}" } }]),
                ),
            )],
        );

        // Même identifiant, casse différente : toujours valide.
        let inventaire = vec![peripherique(
            Famille::EntreeAudio,
            "{0.0.1.00000000}.{aaaa}",
            "Micro USB",
        )];
        let rapport = analyser(&archive, inventaire).unwrap();
        assert!(rapport.a_confirmer.is_empty());
        assert_eq!(rapport.references_valides, 1);
    }

    #[test]
    fn distingue_entree_et_sortie_pour_un_meme_identifiant() {
        // Cas limite : le même identifiant utilisé par une entrée ET une
        // sortie doit donner deux associations distinctes (familles
        // incompatibles entre elles).
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[(
                "S.json",
                collection_type(
                    "S",
                    json!([
                        { "id": "wasapi_input_capture", "name": "In",
                          "settings": { "device_id": "{0.0.1.00000000}.{eeee}" } },
                        { "id": "wasapi_output_capture", "name": "Out",
                          "settings": { "device_id": "{0.0.1.00000000}.{eeee}" } }
                    ]),
                ),
            )],
        );

        let rapport = analyser(&archive, vec![]).unwrap();
        assert_eq!(rapport.a_confirmer.len(), 2);
    }

    #[test]
    fn collection_illisible_ignoree_sans_erreur() {
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[
                ("Cassee.json", "{ pas du json".to_string()),
                (
                    "Valide.json",
                    collection_type(
                        "Valide",
                        json!([{ "id": "wasapi_input_capture", "name": "Micro",
                                 "settings": { "device_id": "{0.0.1.00000000}.{ffff}" } }]),
                    ),
                ),
            ],
        );

        let rapport = analyser(&archive, vec![]).unwrap();
        assert_eq!(rapport.a_confirmer.len(), 1);
    }

    #[test]
    fn collection_anormalement_volumineuse_refusee() {
        let dir = tempfile::tempdir().unwrap();
        let enorme = format!(
            r#"{{ "name": "X", "bourrage": "{}", "sources": [] }}"#,
            "x".repeat((TAILLE_MAX_COLLECTION as usize) + 1024)
        );
        let archive = archive_factice(dir.path(), &[("Enorme.json", enorme)]);

        let e = analyser(&archive, vec![]).unwrap_err();
        assert!(e.contains("anormalement volumineuse"), "{e}");
    }

    #[test]
    fn normalisation_rapproche_les_variantes_unicode() {
        assert_eq!(normaliser_nom("RØDE NT-USB"), "rode nt usb");
        assert_eq!(normaliser_nom("Rode  NT_USB"), "rode nt usb");
        assert_eq!(normaliser_nom("Caméra Éœß"), "camera eoess");
        assert_eq!(normaliser_nom("  --  "), "");
    }

    #[test]
    fn candidats_filtres_par_famille_et_ressemblance_en_tete() {
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[(
                "Stream.json",
                collection_type(
                    "Stream",
                    json!([{ "id": "wasapi_input_capture", "name": "Rode NT USB",
                             "settings": { "device_id": "{0.0.1.00000000}.{aaaa}" } }]),
                ),
            )],
        );
        let inventaire = vec![
            peripherique(Famille::EntreeAudio, "{0.0.1.00000000}.{bbbb}", "Microphone de la webcam"),
            peripherique(Famille::EntreeAudio, "{0.0.1.00000000}.{cccc}", "RØDE NT-USB"),
            peripherique(Famille::SortieAudio, "{0.0.0.00000000}.{dddd}", "Casque"),
        ];

        let rapport = analyser(&archive, inventaire).unwrap();
        let noms: Vec<&str> = rapport.a_confirmer[0]
            .candidats
            .iter()
            .map(|p| p.nom.as_str())
            .collect();
        // Filtré par famille (pas de sortie audio) et la variante Unicode
        // du même nom passe en tête.
        assert_eq!(noms, vec!["RØDE NT-USB", "Microphone de la webcam"]);
    }

    #[test]
    fn webcam_meme_nom_avec_chemin_change_proposee_en_premier() {
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[(
                "S.json",
                collection_type(
                    "S",
                    json!([{ "id": "dshow_input", "name": "Cam",
                             "settings": { "video_device_id": "Webcam C900:ancien#22chemin" } }]),
                ),
            )],
        );
        let inventaire = vec![
            peripherique(Famille::Video, "Carte de capture:x", "Carte de capture"),
            peripherique(Famille::Video, "Webcam C900:nouveau#22chemin", "Webcam C900"),
            peripherique(Famille::Video, "Webcam C920:y", "Webcam C920"),
        ];

        let rapport = analyser(&archive, inventaire).unwrap();
        let noms: Vec<&str> = rapport.a_confirmer[0]
            .candidats
            .iter()
            .map(|p| p.nom.as_str())
            .collect();
        // Même nom, chemin matériel changé : en premier. Puis la webcam de
        // nom proche (mot commun), puis le reste — jamais exclu, seulement
        // ordonné.
        assert_eq!(noms, vec!["Webcam C900", "Webcam C920", "Carte de capture"]);
    }

    #[test]
    fn candidats_sans_ressemblance_ordonnes_alphabetiquement() {
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[(
                "S.json",
                collection_type(
                    "S",
                    json!([{ "id": "wasapi_input_capture", "name": "Micro disparu",
                             "settings": { "device_id": "{0.0.1.00000000}.{aaaa}" } }]),
                ),
            )],
        );
        // Insérés dans le désordre : aucun mot commun avec « Micro disparu ».
        let inventaire = vec![
            peripherique(Famille::EntreeAudio, "{0.0.1.00000000}.{2222}", "Zeta"),
            peripherique(Famille::EntreeAudio, "{0.0.1.00000000}.{1111}", "Alpha"),
        ];

        let rapport = analyser(&archive, inventaire).unwrap();
        let noms: Vec<&str> = rapport.a_confirmer[0]
            .candidats
            .iter()
            .map(|p| p.nom.as_str())
            .collect();
        assert_eq!(noms, vec!["Alpha", "Zeta"]);
    }

    /* ---- Étape 4 : validation et application des choix ---- */

    fn choix(famille: Famille, ancien: &str, nouveau: &str) -> Choix {
        Choix {
            famille,
            ancien_id: ancien.to_string(),
            nouveau_id: nouveau.to_string(),
        }
    }

    fn association(famille: Famille, ancien: &str) -> Association {
        Association {
            famille,
            ancien_id: ancien.to_string(),
            ancien_nom: "Ancien périphérique".to_string(),
            sources: vec![],
            occurrences: 1,
            candidats: vec![],
        }
    }

    /// Écrit un dossier de scènes factice (nom de fichier → contenu JSON).
    fn scenes_factices(dir: &Path, collections: &[(&str, String)]) -> std::path::PathBuf {
        let scenes = dir.join("basic").join("scenes");
        std::fs::create_dir_all(&scenes).unwrap();
        for (nom, contenu) in collections {
            std::fs::write(scenes.join(nom), contenu).unwrap();
        }
        scenes
    }

    #[test]
    fn valider_accepte_un_choix_disponible_meme_avec_une_casse_differente() {
        let inventaire = vec![peripherique(
            Famille::EntreeAudio,
            "{0.0.1.00000000}.{AAAA}",
            "Micro USB",
        )];
        let choix = [choix(
            Famille::EntreeAudio,
            "{0.0.1.00000000}.{vieux}",
            "{0.0.1.00000000}.{aaaa}",
        )];
        let valides = valider_choix(
            &choix,
            &inventaire,
            &[association(
                Famille::EntreeAudio,
                "{0.0.1.00000000}.{VIEUX}",
            )],
        )
        .unwrap();
        assert_eq!(
            valides[0].nouveau_id,
            "{0.0.1.00000000}.{AAAA}",
            "l'identifiant canonique de l'inventaire doit être conservé"
        );
    }

    #[test]
    fn valider_refuse_une_famille_incompatible() {
        // Le périphérique existe, mais comme sortie audio : une entrée audio
        // ne peut pas être remplacée par lui, même si l'interface l'envoie.
        let inventaire = vec![peripherique(
            Famille::SortieAudio,
            "{0.0.0.00000000}.{bbbb}",
            "Casque",
        )];
        let choix = [choix(
            Famille::EntreeAudio,
            "{0.0.1.00000000}.{vieux}",
            "{0.0.0.00000000}.{bbbb}",
        )];
        let e = valider_choix(
            &choix,
            &inventaire,
            &[association(
                Famille::EntreeAudio,
                "{0.0.1.00000000}.{vieux}",
            )],
        )
        .unwrap_err();
        assert!(e.contains("plus disponible"), "{e}");
    }

    #[test]
    fn valider_refuse_un_peripherique_devenu_absent() {
        let e = valider_choix(
            &[choix(Famille::Video, "Webcam C900:x", "Webcam C922:y")],
            &[],
            &[association(Famille::Video, "Webcam C900:x")],
        )
        .unwrap_err();
        assert!(e.contains("plus disponible"), "{e}");
    }

    #[test]
    fn valider_refuse_deux_remplacements_pour_le_meme_identifiant() {
        let inventaire = vec![
            peripherique(Famille::EntreeAudio, "{0.0.1.00000000}.{aaaa}", "Micro A"),
            peripherique(Famille::EntreeAudio, "{0.0.1.00000000}.{bbbb}", "Micro B"),
        ];
        let doublon = [
            choix(Famille::EntreeAudio, "{0.0.1.00000000}.{Vieux}", "{0.0.1.00000000}.{aaaa}"),
            choix(Famille::EntreeAudio, "{0.0.1.00000000}.{vieux}", "{0.0.1.00000000}.{bbbb}"),
        ];
        let e = valider_choix(
            &doublon,
            &inventaire,
            &[association(
                Famille::EntreeAudio,
                "{0.0.1.00000000}.{vieux}",
            )],
        )
        .unwrap_err();
        assert!(e.contains("deux remplacements"), "{e}");
    }

    #[test]
    fn valider_refuse_une_ancienne_reference_qui_nest_pas_a_confirmer() {
        let inventaire = vec![peripherique(
            Famille::EntreeAudio,
            "{0.0.1.00000000}.{nouveau}",
            "Micro neuf",
        )];
        let e = valider_choix(
            &[choix(
                Famille::EntreeAudio,
                "{0.0.1.00000000}.{encore-valide}",
                "{0.0.1.00000000}.{nouveau}",
            )],
            &inventaire,
            &[association(
                Famille::EntreeAudio,
                "{0.0.1.00000000}.{vraiment-absent}",
            )],
        )
        .unwrap_err();
        assert!(e.contains("ne fait pas partie"), "{e}");
    }

    #[test]
    fn applique_remplace_uniquement_le_champ_prevu() {
        let dir = tempfile::tempdir().unwrap();
        let scenes = scenes_factices(
            dir.path(),
            &[(
                "Stream.json",
                json!({
                    "name": "Stream",
                    "sources": [
                        { "id": "wasapi_input_capture", "name": "Micro", "volume": 0.8,
                          "muted": false,
                          "settings": { "device_id": "{0.0.1.00000000}.{vieux}",
                                        "use_device_timing": true } },
                        // Plugin tiers avec une clé ressemblante : intact.
                        { "id": "plugin_tiers", "name": "Deck",
                          "settings": { "device_id": "{0.0.1.00000000}.{vieux}" } }
                    ]
                })
                .to_string(),
            )],
        );

        let n = appliquer(
            &scenes,
            &[choix(
                Famille::EntreeAudio,
                "{0.0.1.00000000}.{VIEUX}",
                "{0.0.1.00000000}.{neuf}",
            )],
        )
        .unwrap();
        assert_eq!(n, 1);

        let racine: Value =
            serde_json::from_str(&std::fs::read_to_string(scenes.join("Stream.json")).unwrap())
                .unwrap();
        let sources = racine["sources"].as_array().unwrap();
        // Champ remplacé, autres réglages conservés.
        assert_eq!(sources[0]["settings"]["device_id"], "{0.0.1.00000000}.{neuf}");
        assert_eq!(sources[0]["settings"]["use_device_timing"], true);
        assert_eq!(sources[0]["volume"], 0.8);
        // La source inconnue est strictement intacte.
        assert_eq!(sources[1]["settings"]["device_id"], "{0.0.1.00000000}.{vieux}");
    }

    #[test]
    fn applique_reecrit_les_deux_champs_video_en_miroir() {
        let dir = tempfile::tempdir().unwrap();
        let scenes = scenes_factices(
            dir.path(),
            &[(
                "S.json",
                json!({
                    "name": "S",
                    "sources": [
                        { "id": "dshow_input", "name": "Cam",
                          "settings": { "video_device_id": "Webcam C900:ancien",
                                        "last_video_device_id": "Webcam C900:ancien",
                                        "audio_device_id": "Micro cam:autre" } }
                    ]
                })
                .to_string(),
            )],
        );

        appliquer(
            &scenes,
            &[choix(Famille::Video, "Webcam C900:ancien", "Webcam C900:nouveau")],
        )
        .unwrap();

        let racine: Value =
            serde_json::from_str(&std::fs::read_to_string(scenes.join("S.json")).unwrap())
                .unwrap();
        let settings = &racine["sources"][0]["settings"];
        assert_eq!(settings["video_device_id"], "Webcam C900:nouveau");
        assert_eq!(settings["last_video_device_id"], "Webcam C900:nouveau");
        // L'audio custom de la webcam n'est jamais touché par le MVP.
        assert_eq!(settings["audio_device_id"], "Micro cam:autre");
    }

    #[test]
    fn applique_remappe_l_audio_global_et_toutes_les_occurrences() {
        let dir = tempfile::tempdir().unwrap();
        let scenes = scenes_factices(
            dir.path(),
            &[
                (
                    "A.json",
                    json!({
                        "name": "A",
                        "AuxAudioDevice1": { "id": "wasapi_input_capture", "name": "Mic/Aux",
                                             "settings": { "device_id": "{0.0.1.00000000}.{vieux}" } },
                        "sources": [
                            { "id": "wasapi_input_capture", "name": "Micro",
                              "settings": { "device_id": "{0.0.1.00000000}.{vieux}" } }
                        ]
                    })
                    .to_string(),
                ),
                (
                    "B.json",
                    json!({
                        "name": "B",
                        "sources": [
                            { "id": "wasapi_input_capture", "name": "Micro bis",
                              "settings": { "device_id": "{0.0.1.00000000}.{vieux}" } }
                        ]
                    })
                    .to_string(),
                ),
            ],
        );

        let n = appliquer(
            &scenes,
            &[choix(
                Famille::EntreeAudio,
                "{0.0.1.00000000}.{vieux}",
                "{0.0.1.00000000}.{neuf}",
            )],
        )
        .unwrap();
        assert_eq!(n, 3, "audio global + 2 sources de scènes");
        for nom in ["A.json", "B.json"] {
            let texte = std::fs::read_to_string(scenes.join(nom)).unwrap();
            assert!(!texte.contains("{vieux}"), "{nom} : {texte}");
        }
    }

    #[test]
    fn applique_sans_choix_ne_touche_a_rien() {
        let dir = tempfile::tempdir().unwrap();
        let contenu = json!({
            "name": "S",
            "sources": [{ "id": "wasapi_input_capture", "name": "Micro",
                          "settings": { "device_id": "{0.0.1.00000000}.{vieux}" } }]
        })
        .to_string();
        let scenes = scenes_factices(dir.path(), &[("S.json", contenu.clone())]);

        assert_eq!(appliquer(&scenes, &[]).unwrap(), 0);
        // Aucun choix (« Laisser inchangé ») : fichier inchangé au bit près.
        assert_eq!(std::fs::read_to_string(scenes.join("S.json")).unwrap(), contenu);
    }

    #[test]
    fn applique_refuse_un_choix_sans_aucune_occurrence() {
        let dir = tempfile::tempdir().unwrap();
        let scenes = scenes_factices(
            dir.path(),
            &[("S.json", collection_type("S", json!([])))],
        );

        let e = appliquer(
            &scenes,
            &[choix(
                Famille::EntreeAudio,
                "{0.0.1.00000000}.{fantome}",
                "{0.0.1.00000000}.{neuf}",
            )],
        )
        .unwrap_err();
        assert!(e.contains("aucune source correspondante"), "{e}");
    }

    #[test]
    fn chemin_suspect_sous_scenes_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let archive = archive_factice(
            dir.path(),
            &[("../evil.json", collection_type("X", json!([])))],
        );

        let e = analyser(&archive, vec![]).unwrap_err();
        assert!(e.contains("Archive invalide ou malveillante"), "{e}");
    }
}
