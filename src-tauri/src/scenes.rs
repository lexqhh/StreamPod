//! Analyse des collections de scènes OBS : extraction et réécriture des
//! chemins d'assets (images, vidéos, sons, fichiers locaux de sources
//! navigateur, scripts…) référencés par les JSON de scènes.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Normalise un chemin pour comparaison : slashes avant, minuscules.
pub fn normalize_path(p: &str) -> String {
    p.replace('\\', "/").to_lowercase()
}

/// Une chaîne ressemble-t-elle à un chemin absolu Windows (C:\... ou C:/...) ?
fn looks_like_windows_path(s: &str) -> bool {
    let bytes = s.as_bytes();
    s.len() >= 4
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

/// Sources dont un champ liste des fichiers OU des dossiers (`[{ "value" }]`).
const SOURCES_A_LISTE: &[(&str, &str)] = &[("slideshow", "files"), ("vlc_source", "playlist")];

/// Références relevées dans les collections de scènes.
#[derive(Debug, Default)]
pub struct References {
    /// Chaînes ressemblant à un chemin absolu Windows (existant ou non).
    pub chemins: BTreeSet<String>,
    /// Entrées de diaporama ou de playlist VLC (fichier ou dossier).
    pub listes_media: BTreeSet<String>,
    /// Polices des sources texte (`settings.font.face`).
    pub polices: BTreeSet<String>,
    /// Sources navigateur dont l'URL pointe vers le web : elle peut contenir
    /// un token privé (`…/overlay/<id>/<TOKEN>` chez StreamElements…).
    pub sources_navigateur: usize,
}

/// Parcourt récursivement une collection de scènes et relève ses références.
pub fn analyser_collection(value: &Value, refs: &mut References) {
    match value {
        Value::String(s) => {
            if looks_like_windows_path(s) && !s.contains('\n') {
                refs.chemins.insert(s.clone());
            }
        }
        Value::Array(items) => {
            for item in items {
                analyser_collection(item, refs);
            }
        }
        Value::Object(map) => {
            let id = map.get("id").and_then(Value::as_str).unwrap_or_default();
            let settings = map.get("settings");
            let url = settings.and_then(|s| s.get("url")).and_then(Value::as_str);
            if id == "browser_source"
                && url.is_some_and(|u| {
                    let u = u.to_ascii_lowercase();
                    u.starts_with("http://") || u.starts_with("https://")
                })
            {
                refs.sources_navigateur += 1;
            }
            if let Some((_, champ)) = SOURCES_A_LISTE.iter().find(|(t, _)| id.starts_with(t)) {
                refs.listes_media.extend(
                    settings
                        .and_then(|s| s.get(*champ))
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|e| e.get("value").and_then(Value::as_str))
                        .filter(|v| looks_like_windows_path(v))
                        .map(str::to_string),
                );
            }
            if id.starts_with("text_gdiplus") || id.starts_with("text_ft2_source") {
                if let Some(face) = settings
                    .and_then(|s| s.pointer("/font/face"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|f| !f.is_empty())
                {
                    refs.polices.insert(face.to_string());
                }
            }
            for v in map.values() {
                analyser_collection(v, refs);
            }
        }
        _ => {}
    }
}

/// Réécrit dans un JSON de scènes toutes les chaînes correspondant à un
/// ancien chemin d'asset (comparaison insensible à la casse et aux slashes)
/// vers le nouveau chemin. Retourne le nombre de remplacements.
pub fn rewrite_asset_paths(value: &mut Value, mapping: &BTreeMap<String, String>) -> usize {
    match value {
        Value::String(s) => {
            if looks_like_windows_path(s) {
                if let Some(new_path) = mapping.get(&normalize_path(s)) {
                    *s = new_path.clone();
                    return 1;
                }
            }
            0
        }
        Value::Array(items) => items
            .iter_mut()
            .map(|item| rewrite_asset_paths(item, mapping))
            .sum(),
        Value::Object(map) => map
            .values_mut()
            .map(|v| rewrite_asset_paths(v, mapping))
            .sum(),
        _ => 0,
    }
}

/// Extensions de fichiers exécutables par Windows : jamais archivées ni
/// extraites comme assets (une archive est une donnée non fiable).
const EXTENSIONS_EXECUTABLES: &[&str] = &[
    "dll", "exe", "bat", "cmd", "com", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh",
    "msi", "msp", "scr", "cpl", "hta", "pif", "lnk", "reg", "jar", "sys",
];

/// Extension (en minuscules) du dernier composant d'un chemin, `.env` compris.
pub fn extension_de(chemin: &str) -> String {
    let nom = chemin.rsplit(['/', '\\']).next().unwrap_or(chemin);
    nom.rsplit_once('.')
        .map(|(_, ext)| ext.trim_end_matches([' ', '.']).to_lowercase())
        .unwrap_or_default()
}

pub fn est_executable(chemin: &str) -> bool {
    EXTENSIONS_EXECUTABLES.contains(&extension_de(chemin).as_str())
}

/// Neutralise les scripts d'une collection (`modules["scripts-tool"]`) : OBS
/// les exécute à son lancement. Retourne leurs chemins pour une réactivation
/// manuelle (Outils → Scripts).
pub fn retirer_scripts(collection: &mut Value) -> Vec<String> {
    let Some(scripts) = collection
        .get_mut("modules")
        .and_then(Value::as_object_mut)
        .and_then(|m| m.get_mut("scripts-tool"))
    else {
        return Vec::new();
    };
    let chemins = scripts
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.get("path").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    *scripts = Value::Array(Vec::new());
    chemins
}

/// Nom de fichier sûr pour l'archive (dernier composant du chemin).
pub fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "fichier".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn analyse(scene: &Value) -> References {
        let mut refs = References::default();
        analyser_collection(scene, &mut refs);
        refs
    }

    #[test]
    fn releve_chemins_listes_media_et_polices() {
        let refs = analyse(&json!({
            "sources": [
                { "id": "image_source", "settings": { "file": "C:/x/overlay.png" } },
                { "id": "ffmpeg_source", "settings": { "local_file": "C:/n/existe/pas.mp4" } },
                { "id": "slideshow_v2", "settings": { "files": [
                    { "value": "C:/photos", "hidden": false }, { "value": "pas un chemin" } ] } },
                { "id": "vlc_source", "settings": { "playlist": [ { "value": "D:/clips" } ] } },
                { "id": "text_gdiplus_v3", "settings": { "font": { "face": "Bebas Neue", "size": 48 } } },
                { "id": "text_ft2_source_v2", "settings": { "font": { "face": " " } } },
                { "settings": { "text": "pas un chemin" } }
            ]
        }));
        assert_eq!(
            refs.chemins.iter().map(String::as_str).collect::<Vec<_>>(),
            [
                "C:/n/existe/pas.mp4",
                "C:/photos",
                "C:/x/overlay.png",
                "D:/clips"
            ]
        );
        assert_eq!(
            refs.listes_media
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["C:/photos", "D:/clips"]
        );
        assert_eq!(
            refs.polices.iter().map(String::as_str).collect::<Vec<_>>(),
            ["Bebas Neue"]
        );
    }

    #[test]
    fn les_sources_navigateur_web_sont_comptees_meme_imbriquees() {
        let refs = analyse(&json!({
            "sources": [
                { "id": "browser_source",
                  "settings": { "url": "https://streamelements.com/overlay/abc/TOKEN" } },
                { "id": "browser_source",
                  "settings": { "url": "HTTP://exemple.test/alertes" } },
                { "id": "group", "settings": { "items": [
                    { "id": "browser_source",
                      "settings": { "url": "https://streamlabs.com/widget/xyz" } }
                ] } },
                { "id": "image_source", "settings": { "file": "C:/logo.png" } }
            ]
        }));
        assert_eq!(refs.sources_navigateur, 3);
    }

    #[test]
    fn les_sources_navigateur_sans_url_web_ne_sont_pas_comptees() {
        let refs = analyse(&json!({
            "sources": [
                { "id": "browser_source", "settings": { "is_local_file": true,
                  "local_file": "C:/overlay/index.html", "url": "" } },
                { "id": "browser_source", "settings": {} },
                { "id": "browser_source" },
                { "id": "text_gdiplus", "settings": { "url": "https://pas-un-navigateur.test" } }
            ]
        }));
        assert_eq!(refs.sources_navigateur, 0);
    }

    #[test]
    fn extensions_executables_reconnues() {
        assert!(est_executable("C:/x/Evil.DLL"));
        assert!(est_executable("assets/0/run.bat"));
        assert!(est_executable(r"C:\x\script.ps1"));
        assert!(!est_executable("C:/x/overlay.png"));
        assert!(!est_executable("C:/x/chat.lua"));
        assert!(!est_executable("C:/x/sans-extension"));
        assert_eq!(extension_de("C:/x/.env"), "env");
        assert_eq!(extension_de("C:/x/service.json.bak"), "bak");
    }

    #[test]
    fn les_scripts_sont_retires_et_listes() {
        let mut collection = json!({
            "modules": {
                "scripts-tool": [
                    { "path": "C:/scripts/chat.lua", "settings": {} },
                    { "path": "C:/scripts/alertes.py" }
                ],
                "auto-scene-switcher": { "active": false }
            }
        });
        let scripts = retirer_scripts(&mut collection);
        assert_eq!(scripts, ["C:/scripts/chat.lua", "C:/scripts/alertes.py"]);
        assert_eq!(collection["modules"]["scripts-tool"], json!([]));
        assert_eq!(
            collection["modules"]["auto-scene-switcher"]["active"],
            false
        );
        assert!(retirer_scripts(&mut json!({ "sources": [] })).is_empty());
    }

    #[test]
    fn reecrit_les_chemins_quelle_que_soit_la_casse_et_les_slashes() {
        let mut scene = json!({
            "sources": [
                { "settings": { "file": "C:\\Users\\Streamer\\Overlay.PNG" } },
                { "playlist": [ { "value": "C:/Users/Streamer/intro.mp4" } ] }
            ]
        });

        let mut mapping = BTreeMap::new();
        mapping.insert(
            "c:/users/streamer/overlay.png".to_string(),
            "D:/Assets/Overlay.PNG".to_string(),
        );
        mapping.insert(
            "c:/users/streamer/intro.mp4".to_string(),
            "D:/Assets/intro.mp4".to_string(),
        );

        let n = rewrite_asset_paths(&mut scene, &mapping);
        assert_eq!(n, 2);
        assert_eq!(
            scene["sources"][0]["settings"]["file"],
            "D:/Assets/Overlay.PNG"
        );
        assert_eq!(
            scene["sources"][1]["playlist"][0]["value"],
            "D:/Assets/intro.mp4"
        );
    }
}
