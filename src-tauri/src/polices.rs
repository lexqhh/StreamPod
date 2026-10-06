//! Polices installées sur ce PC, pour signaler celles qu'une sauvegarde
//! utilise mais qui manquent (le rendu des sources texte changerait sans
//! le moindre message).

use std::collections::BTreeSet;

/// Polices de `polices` absentes de ce PC. Liste vide si la détection est
/// impossible : pas d'alerte sur un doute. Surchargeable via
/// STREAMPOD_POLICES (noms séparés par `;`, tests).
pub fn polices_absentes(polices: &[String]) -> Vec<String> {
    if polices.is_empty() {
        return Vec::new();
    }
    let installees = match std::env::var("STREAMPOD_POLICES") {
        Ok(liste) => liste.split(';').map(|n| n.trim().to_lowercase()).collect(),
        Err(_) => match polices_systeme() {
            Some(noms) => noms,
            None => return Vec::new(),
        },
    };
    polices
        .iter()
        .filter(|p| !installees.contains(&p.trim().to_lowercase()))
        .cloned()
        .collect()
}

/// Noms de familles (en minuscules) de la collection système DirectWrite,
/// polices installées par utilisateur comprises. Les noms Win32 (« Segoe UI
/// Black », utilisés par `text_gdiplus`) s'ajoutent aux noms typographiques.
#[cfg(windows)]
fn polices_systeme() -> Option<BTreeSet<String>> {
    use windows::core::BOOL;
    use windows::Win32::Graphics::DirectWrite::{
        DWriteCreateFactory, IDWriteFactory, IDWriteLocalizedStrings, DWRITE_FACTORY_TYPE_SHARED,
        DWRITE_INFORMATIONAL_STRING_WIN32_FAMILY_NAMES,
    };

    fn ajouter(noms: &mut BTreeSet<String>, chaines: &IDWriteLocalizedStrings) {
        unsafe {
            for k in 0..chaines.GetCount() {
                let Ok(len) = chaines.GetStringLength(k) else {
                    continue;
                };
                let mut buf = vec![0u16; len as usize + 1];
                if chaines.GetString(k, &mut buf).is_ok() {
                    noms.insert(String::from_utf16_lossy(&buf[..len as usize]).to_lowercase());
                }
            }
        }
    }

    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let mut collection = None;
        factory
            .GetSystemFontCollection(&mut collection, false)
            .ok()?;
        let collection = collection?;
        let mut noms = BTreeSet::new();
        for i in 0..collection.GetFontFamilyCount() {
            let Ok(famille) = collection.GetFontFamily(i) else {
                continue;
            };
            if let Ok(chaines) = famille.GetFamilyNames() {
                ajouter(&mut noms, &chaines);
            }
            for j in 0..famille.GetFontCount() {
                let Ok(police) = famille.GetFont(j) else {
                    continue;
                };
                let mut chaines = None;
                let mut existe = BOOL(0);
                if police
                    .GetInformationalStrings(
                        DWRITE_INFORMATIONAL_STRING_WIN32_FAMILY_NAMES,
                        &mut chaines,
                        &mut existe,
                    )
                    .is_ok()
                    && existe.as_bool()
                {
                    if let Some(chaines) = chaines {
                        ajouter(&mut noms, &chaines);
                    }
                }
            }
        }
        Some(noms)
    }
}

#[cfg(not(windows))]
fn polices_systeme() -> Option<BTreeSet<String>> {
    None
}

#[cfg(test)]
mod tests {
    use super::polices_systeme;

    /// Lecture seule : toute installation Windows a au moins Arial.
    #[cfg(windows)]
    #[test]
    fn la_collection_systeme_contient_arial() {
        let noms = polices_systeme().expect("DirectWrite disponible");
        assert!(noms.contains("arial"), "{} familles lues", noms.len());
    }
}
