//! Analyse des collections de scènes OBS : extraction et réécriture des
//! chemins d'assets (images, vidéos, sons, fichiers locaux de sources
//! navigateur, scripts…) référencés par les JSON de scènes.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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

/// Parcourt récursivement un JSON de scènes et collecte tous les chemins
/// absolus qui pointent vers un fichier existant sur le disque.
pub fn collect_asset_paths(value: &Value, out: &mut BTreeSet<PathBuf>) {
    match value {
        Value::String(s) => {
            if looks_like_windows_path(s) {
                let p = PathBuf::from(s);
                if p.is_file() {
                    out.insert(p);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_asset_paths(item, out);
            }
        }
        Value::Object(map) => {
            for v in map.values() {
                collect_asset_paths(v, out);
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

    #[test]
    fn collecte_les_chemins_existants_seulement() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("overlay.png");
        std::fs::write(&existing, b"png").unwrap();
        let existing_str = existing.to_string_lossy().replace('\\', "/");

        let scene = json!({
            "sources": [
                { "settings": { "file": existing_str } },
                { "settings": { "local_file": "C:/n/existe/pas.png" } },
                { "settings": { "text": "pas un chemin" } }
            ]
        });

        let mut out = BTreeSet::new();
        collect_asset_paths(&scene, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out.contains(&existing));
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
        assert_eq!(scene["sources"][1]["playlist"][0]["value"], "D:/Assets/intro.mp4");
    }
}
