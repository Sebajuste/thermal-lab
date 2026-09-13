//! La langue de l'interface, decidee une fois par Windows.
//!
//! L'anglais est la langue par defaut : c'est celle que comprend le plus grand nombre,
//! et celle des outils tiers vers lesquels l'application renvoie. Le francais n'apparait
//! que sur un Windows affiche en francais — pas sur un Windows anglais dont le format
//! regional est francais, ce qui n'est pas la meme chose et ne dit rien de la langue
//! qu'on lit.
//!
//! Aucun reglage : la langue du systeme est le seul signal, et un panneau ouvert
//! quelques secondes n'est pas l'endroit ou choisir sa langue.

use std::sync::OnceLock;

use serde::Serialize;

mod win {
    pub use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
}

/// Identifiant de langue principale du francais, dans les 10 bits bas d'un `LANGID`.
/// Toutes les variantes regionales — fr-FR, fr-CA, fr-BE — le partagent, et toutes
/// doivent donner du francais.
const LANG_FRENCH: u16 = 0x0c;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Fr,
}

/// `GetUserDefaultUILanguage` rend la langue d'*affichage* de la session, celle des
/// menus de Windows — et non `GetUserDefaultLocaleName`, qui rend le format regional.
pub fn lang() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(|| {
        let langid = unsafe { win::GetUserDefaultUILanguage() };
        if langid & 0x3ff == LANG_FRENCH {
            Lang::Fr
        } else {
            Lang::En
        }
    })
}

/// Choisit entre deux redactions. Les deux branches gardent le meme type, ce qui laisse
/// passer aussi bien deux litteraux qu'un `format!` de chaque cote.
#[macro_export]
macro_rules! t {
    ($en:expr, $fr:expr $(,)?) => {
        match $crate::i18n::lang() {
            $crate::i18n::Lang::En => $en,
            $crate::i18n::Lang::Fr => $fr,
        }
    };
}
