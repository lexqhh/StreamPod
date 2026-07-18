//! Nettoyage des données sensibles avant sauvegarde.
//!
//! Règle absolue : la clé de stream et les identifiants/tokens de comptes
//! connectés (Twitch, Kick, YouTube…) ne doivent JAMAIS se retrouver dans
//! un fichier .obsbackup.

use serde_json::Value;

/// Noms de champs JSON supprimés par correspondance exacte, en complément de
/// la détection par sous-chaîne de `json_key_is_sensitive`.
const SENSITIVE_JSON_KEYS: &[&str] = &["username", "connected_account"];

/// Une clé JSON est-elle sensible ? Détection par sous-chaîne car on assainit
/// des JSON de plugins tiers arbitraires (serverPassword, apiKey, auth_token…),
/// pas seulement service.json. Les JSON de scènes (basic/scenes/**) ne passent
/// JAMAIS par ici : leurs champs `key`/`hotkeys` (raccourcis clavier) sont
/// légitimes et conservés.
fn json_key_is_sensitive(key: &str) -> bool {
    let k = key.to_lowercase();
    if SENSITIVE_JSON_KEYS.contains(&k.as_str()) {
        return true;
    }
    if k.contains("password")
        || k.contains("token")
        || k.contains("secret")
        || k.contains("cookie")
        || k.contains("oauth")
    {
        return true;
    }
    // « key » en sous-chaîne pure supprimerait des champs légitimes
    // (hotkey, keyframe, keyboard…) : on ne retient que les clés qui se
    // TERMINENT par « key » (key, stream_key, apiKey, StreamKey…), en
    // épargnant les raccourcis clavier (hotkey).
    k.ends_with("key") && !k.contains("hotkey")
}

/// Une clé INI est-elle sensible ? (RefreshToken=, StreamKey=, …)
fn ini_key_is_sensitive(key: &str) -> bool {
    let k = key.trim().to_lowercase();
    k == "key"
        || k == "stream_key"
        || k == "streamkey"
        || k.contains("token")
        || k.contains("secret")
        || k.contains("password")
        || k.contains("cookie")
        || k.contains("oauth")
}

/// Supprime récursivement tous les champs sensibles d'un JSON.
/// Retourne le nombre de champs supprimés.
pub fn sanitize_json(value: &mut Value) -> usize {
    match value {
        Value::Object(map) => {
            let before = map.len();
            map.retain(|k, _| !json_key_is_sensitive(k));
            let mut removed = before - map.len();
            for v in map.values_mut() {
                removed += sanitize_json(v);
            }
            removed
        }
        Value::Array(items) => items.iter_mut().map(sanitize_json).sum(),
        _ => 0,
    }
}

/// Supprime les lignes sensibles d'un fichier INI (basic.ini, global.ini…).
pub fn sanitize_ini(content: &str) -> String {
    content
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('[') || trimmed.starts_with(';') || trimmed.starts_with('#') {
                return true;
            }
            match trimmed.split_once('=') {
                Some((key, _)) => !ini_key_is_sensitive(key),
                None => true,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Un chemin relatif (dans le dossier de config OBS) doit-il être exclu de
/// la sauvegarde ? `rel` utilise des slashes avant et est en minuscules.
pub fn is_excluded_config_path(rel: &str) -> bool {
    const EXCLUDED_PREFIXES: &[&str] = &[
        "logs",
        "crashes",
        "profiler_data",
        "updates",
        // Cookies et sessions des docks navigateur (connexions Twitch/YT).
        "plugin_config/obs-browser",
    ];
    EXCLUDED_PREFIXES
        .iter()
        .any(|p| rel == *p || rel.starts_with(&format!("{p}/")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn supprime_la_cle_de_stream_du_service_json() {
        let mut service = json!({
            "type": "rtmp_common",
            "settings": {
                "service": "Twitch",
                "server": "auto",
                "key": "live_123456_SECRETSECRET",
                "bwtest": false
            }
        });
        let removed = sanitize_json(&mut service);
        assert_eq!(removed, 1);
        assert!(service["settings"].get("key").is_none());
        assert_eq!(service["settings"]["service"], "Twitch");
        let text = serde_json::to_string(&service).unwrap();
        assert!(!text.contains("SECRET"));
    }

    #[test]
    fn supprime_les_tokens_oauth_imbriques() {
        let mut v = json!({
            "settings": {
                "connected_account": { "name": "streamer" },
                "auth": { "access_token": "abc", "refresh_token": "def", "expires": 12 }
            }
        });
        sanitize_json(&mut v);
        let text = serde_json::to_string(&v).unwrap();
        assert!(!text.contains("abc"));
        assert!(!text.contains("def"));
        assert!(!text.contains("connected_account"));
        assert_eq!(v["settings"]["auth"]["expires"], 12);
    }

    #[test]
    fn supprime_le_mot_de_passe_du_serveur_obs_websocket() {
        let mut v = json!({
            "server_enabled": true,
            "server_password": "WSPASS_ULTRASECRET",
            "server_port": 4455
        });
        let removed = sanitize_json(&mut v);
        assert_eq!(removed, 1);
        assert!(v.get("server_password").is_none());
        assert_eq!(v["server_enabled"], true);
        assert_eq!(v["server_port"], 4455);
        let text = serde_json::to_string(&v).unwrap();
        assert!(!text.contains("ULTRASECRET"));
    }

    #[test]
    fn detecte_les_cles_sensibles_en_camel_case_et_variantes() {
        let mut v = json!({
            "serverPassword": "SECRET1",
            "apiKey": "SECRET2",
            "auth_password": "SECRET3",
            "stream_secret": "SECRET4",
            "server_port": 4455
        });
        let removed = sanitize_json(&mut v);
        assert_eq!(removed, 4);
        assert_eq!(v["server_port"], 4455);
        let text = serde_json::to_string(&v).unwrap();
        assert!(!text.contains("SECRET1"));
        assert!(!text.contains("SECRET2"));
        assert!(!text.contains("SECRET3"));
        assert!(!text.contains("SECRET4"));
    }

    #[test]
    fn conserve_les_champs_benins_contenant_key() {
        let mut v = json!({
            "hotkey": "OBS_KEY_F1",
            "keyframe_interval": 2,
            "keyboard_layout": "azerty",
            "monitor": 1
        });
        let removed = sanitize_json(&mut v);
        assert_eq!(removed, 0);
        assert_eq!(v["hotkey"], "OBS_KEY_F1");
        assert_eq!(v["keyframe_interval"], 2);
        assert_eq!(v["keyboard_layout"], "azerty");
    }

    #[test]
    fn nettoie_les_lignes_ini_sensibles_et_garde_le_reste() {
        let ini = "[General]\nName=MonProfil\n[Twitch]\nRefreshToken=xyz\nOAuthSecret=abc\nDockState=ok\n[Hotkeys]\nHotkeyFocusBehavior=0\n";
        let clean = sanitize_ini(ini);
        assert!(!clean.contains("xyz"));
        assert!(!clean.contains("abc"));
        assert!(clean.contains("Name=MonProfil"));
        assert!(clean.contains("DockState=ok"));
        assert!(clean.contains("HotkeyFocusBehavior=0"));
    }

    #[test]
    fn exclut_les_dossiers_sensibles_ou_inutiles() {
        assert!(is_excluded_config_path("logs/2026-07-16.txt"));
        assert!(is_excluded_config_path("plugin_config/obs-browser/obs_profile_cookies/cookies.sqlite"));
        assert!(!is_excluded_config_path("plugin_config/obs-websocket/config.json"));
        assert!(!is_excluded_config_path("basic/scenes/scenes.json"));
    }
}
