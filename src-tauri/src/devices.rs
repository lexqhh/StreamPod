//! Inventaire des périphériques audio et vidéo actifs de ce PC, au format
//! exact des identifiants qu'OBS écrit dans ses collections de scènes
//! (voir docs/FORMATS-OBS.md) :
//! - audio : identifiant d'endpoint MMDevice `{0.0.X.00000000}.{guid}` ;
//! - vidéo : `NomConvivial:CheminInterface` encodés façon win-dshow
//!   (`#` → `#22`, `:` → `#3A`).

use serde::{Deserialize, Serialize};

/// Valeur spéciale OBS « périphérique audio par défaut du système » :
/// référence toujours valide, jamais proposée au remappage.
pub const AUDIO_PAR_DEFAUT: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Famille {
    EntreeAudio,
    SortieAudio,
    Video,
}

/// Un périphérique présent et actif sur ce PC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peripherique {
    pub famille: Famille,
    /// Identifiant au format OBS, comparable tel quel aux champs des scènes.
    pub id: String,
    /// Nom convivial affichable.
    pub nom: String,
}

/// Encodage des identifiants vidéo du plugin win-dshow d'OBS (encode_dstr) :
/// le `:` restant est l'unique séparateur nom/chemin.
pub fn encoder_dstr(s: &str) -> String {
    s.replace('#', "#22").replace(':', "#3A")
}

/// Opération inverse de [`encoder_dstr`].
pub fn decoder_dstr(s: &str) -> String {
    s.replace("#3A", ":").replace("#22", "#")
}

/// Nom convivial contenu dans un identifiant vidéo OBS (`nom:chemin`).
pub fn nom_video(id: &str) -> Option<String> {
    let (nom, _chemin) = id.split_once(':')?;
    if nom.is_empty() {
        return None;
    }
    Some(decoder_dstr(nom))
}

/// Inventaire des périphériques actifs de ce PC.
///
/// Surchargeable via la variable d'environnement STREAMPOD_DEVICES_JSON (tests) :
/// chemin d'un fichier JSON `[{"famille": "...", "id": "...", "nom": "..."}]`.
/// Les tests ne doivent jamais dépendre des périphériques réels.
pub fn inventaire() -> Result<Vec<Peripherique>, String> {
    if let Ok(path) = std::env::var("STREAMPOD_DEVICES_JSON") {
        return inventaire_depuis_fichier(&path);
    }
    inventaire_reel()
}

fn inventaire_depuis_fichier(path: &str) -> Result<Vec<Peripherique>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("Lecture de l'inventaire de test {path} : {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("Inventaire de test illisible : {e}"))
}

#[cfg(windows)]
pub fn inventaire_reel() -> Result<Vec<Peripherique>, String> {
    win::inventaire()
}

#[cfg(not(windows))]
pub fn inventaire_reel() -> Result<Vec<Peripherique>, String> {
    Err("L'inventaire des périphériques n'est disponible que sous Windows.".to_string())
}

#[cfg(windows)]
mod win {
    use super::{encoder_dstr, Famille, Peripherique};
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        eCapture, eRender, EDataFlow, IMMDeviceEnumerator, MMDeviceEnumerator,
        DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::Media::DirectShow::ICreateDevEnum;
    use windows::Win32::System::Com::StructuredStorage::IPropertyBag;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_MULTITHREADED, STGM_READ,
    };
    use windows::Win32::System::Variant::VARIANT;

    // GUID documentés (uuids.h) : énumérateur système et catégorie
    // « périphériques d'entrée vidéo » de DirectShow.
    const CLSID_SYSTEM_DEVICE_ENUM: windows::core::GUID =
        windows::core::GUID::from_u128(0x62BE5D10_60EB_11d0_BD3B_00A0C911CE86);
    const CLSID_VIDEO_INPUT_DEVICE_CATEGORY: windows::core::GUID =
        windows::core::GUID::from_u128(0x860BB310_5D01_11d0_BD3B_00A0C911CE86);

    fn err(context: &str, e: impl std::fmt::Display) -> String {
        format!("{context} : {e}")
    }

    /// Garde d'initialisation COM pour le thread courant.
    struct Com;

    impl Com {
        fn init() -> Result<Self, String> {
            // S_FALSE (déjà initialisé) est un succès ; seul un vrai échec
            // (par ex. mode d'appartement incompatible) est bloquant.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_err() {
                return Err(err("Initialisation COM", hr));
            }
            Ok(Com)
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    pub fn inventaire() -> Result<Vec<Peripherique>, String> {
        let _com = Com::init()?;
        let mut out = Vec::new();
        audio(eCapture, Famille::EntreeAudio, &mut out)?;
        audio(eRender, Famille::SortieAudio, &mut out)?;
        video(&mut out)?;
        Ok(out)
    }

    /// Endpoints audio actifs via l'API MMDevice. `IMMDevice::GetId` retourne
    /// exactement l'identifiant qu'OBS écrit dans `settings.device_id`.
    fn audio(
        flux: EDataFlow,
        famille: Famille,
        out: &mut Vec<Peripherique>,
    ) -> Result<(), String> {
        let ctx = "Énumération des périphériques audio";
        unsafe {
            let enumerateur: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| err(ctx, e))?;
            let collection = enumerateur
                .EnumAudioEndpoints(flux, DEVICE_STATE_ACTIVE)
                .map_err(|e| err(ctx, e))?;
            let count = collection.GetCount().map_err(|e| err(ctx, e))?;
            for i in 0..count {
                let Ok(device) = collection.Item(i) else { continue };
                let Ok(id_ptr) = device.GetId() else { continue };
                let id = id_ptr.to_string().unwrap_or_default();
                CoTaskMemFree(Some(id_ptr.as_ptr() as *const _));
                if id.is_empty() {
                    continue;
                }
                let nom = device
                    .OpenPropertyStore(STGM_READ)
                    .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName))
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                out.push(Peripherique {
                    famille,
                    id,
                    nom: if nom.is_empty() { "Périphérique audio".into() } else { nom },
                });
            }
        }
        Ok(())
    }

    /// Périphériques de capture vidéo via DirectShow, identifiés comme le fait
    /// OBS : `encode(FriendlyName) + ":" + encode(DevicePath)`.
    fn video(out: &mut Vec<Peripherique>) -> Result<(), String> {
        let ctx = "Énumération des périphériques vidéo";
        unsafe {
            let dev_enum: ICreateDevEnum =
                CoCreateInstance(&CLSID_SYSTEM_DEVICE_ENUM, None, CLSCTX_ALL)
                    .map_err(|e| err(ctx, e))?;
            let mut moniker_enum = None;
            dev_enum
                .CreateClassEnumerator(&CLSID_VIDEO_INPUT_DEVICE_CATEGORY, &mut moniker_enum, 0)
                .map_err(|e| err(ctx, e))?;
            // Aucune caméra branchée : CreateClassEnumerator réussit mais ne
            // fournit pas d'énumérateur.
            let Some(moniker_enum) = moniker_enum else {
                return Ok(());
            };
            loop {
                let mut monikers = [None];
                if moniker_enum.Next(&mut monikers, None).is_err() {
                    break;
                }
                let Some(moniker) = monikers[0].take() else { break };
                let Ok(bag) = moniker.BindToStorage::<_, _, IPropertyBag>(None, None) else {
                    continue;
                };
                let Some(nom) = lire_propriete(&bag, "FriendlyName") else {
                    continue;
                };
                // Certains périphériques virtuels n'ont pas de DevicePath :
                // OBS écrit alors `nom:` (chemin vide), on fait pareil.
                let chemin = lire_propriete(&bag, "DevicePath").unwrap_or_default();
                out.push(Peripherique {
                    famille: Famille::Video,
                    id: format!("{}:{}", encoder_dstr(&nom), encoder_dstr(&chemin)),
                    nom,
                });
            }
        }
        Ok(())
    }

    fn lire_propriete(bag: &IPropertyBag, nom: &str) -> Option<String> {
        unsafe {
            let mut variant = VARIANT::default();
            let nom_wide = windows::core::HSTRING::from(nom);
            bag.Read(&nom_wide, &mut variant, None).ok()?;
            let texte = variant.to_string();
            if texte.is_empty() {
                None
            } else {
                Some(texte)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodage_dstr_conforme_a_obs() {
        // Règles exactes du plugin win-dshow : `#` → `#22`, `:` → `#3A`.
        assert_eq!(
            encoder_dstr(r"\\?\usb#vid_1234&mi_00#7&abc#{65e8773d}\global"),
            r"\\?\usb#22vid_1234&mi_00#227&abc#22{65e8773d}\global"
        );
        assert_eq!(encoder_dstr("Webcam: Pro"), "Webcam#3A Pro");
    }

    #[test]
    fn decodage_dstr_est_l_inverse() {
        let original = r"Nom:Bizarre#1\\?\usb#vid#{guid}";
        assert_eq!(decoder_dstr(&encoder_dstr(original)), original);
    }

    #[test]
    fn nom_video_extrait_et_decode_la_partie_nom() {
        let id = "Webcam Exemple C900:\\\\?\\usb#22vid_1234#22{65e8773d}\\global";
        assert_eq!(nom_video(id).as_deref(), Some("Webcam Exemple C900"));
        // Nom contenant un `:` encodé.
        assert_eq!(nom_video("Cam#3A Pro:chemin").as_deref(), Some("Cam: Pro"));
        // Pas de séparateur ou nom vide : pas de nom exploitable.
        assert_eq!(nom_video("sans-separateur"), None);
        assert_eq!(nom_video(":chemin-seul"), None);
    }

    #[test]
    fn inventaire_factice_lu_depuis_un_fichier() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devices.json");
        std::fs::write(
            &path,
            r#"[
              {"famille": "entree_audio", "id": "{0.0.1.00000000}.{aaaa}", "nom": "Micro USB"},
              {"famille": "sortie_audio", "id": "{0.0.0.00000000}.{bbbb}", "nom": "Casque"},
              {"famille": "video", "id": "Webcam:chemin", "nom": "Webcam"}
            ]"#,
        )
        .unwrap();

        let devices = inventaire_depuis_fichier(&path.to_string_lossy()).unwrap();
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].famille, Famille::EntreeAudio);
        assert_eq!(devices[1].famille, Famille::SortieAudio);
        assert_eq!(devices[2].famille, Famille::Video);
    }

    #[test]
    fn inventaire_factice_illisible_message_francais() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devices.json");
        std::fs::write(&path, "pas du json").unwrap();
        let e = inventaire_depuis_fichier(&path.to_string_lossy()).unwrap_err();
        assert!(e.contains("illisible"), "{e}");
    }
}
