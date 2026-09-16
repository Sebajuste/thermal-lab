//! Les profils d'optimisation : des cibles nommees, levier par levier.
//!
//! Un profil est une donnee, pas un etat. Ce que la machine applique se relit dans le
//! schema d'alimentation ; ce module dit seulement a quel profil connu ces valeurs
//! correspondent, s'il y en a un. Des valeurs qui ne correspondent a aucun — posees par
//! un outil tiers, ou a la main — ne sont pas une erreur : c'est un etat personnalise,
//! et l'interface le dit plutot que d'afficher un profil qui ne s'applique plus.
//!
//! L'interrupteur principal ne connait que deux gestes : appliquer un profil, ou rendre
//! la machine. Le choix du profil est une autre question, posee a cote de lui.

use serde::{Deserialize, Serialize};

use crate::power::Targets;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProfileId {
    /// Turbo interdit par l'un et l'autre levier : le bridage d'origine de l'application.
    Capped,
}

/// Ce qu'un profil fait payer, et donc comment en mesurer l'effet.
///
/// Un arbitrage retire des performances pour gagner des degres : sa mesure compare deux
/// phases a charge comparable. Une suppression de gaspillage ne coute rien — elle agit
/// sur une consommation sans travail — et sa mesure est une energie economisee au repos,
/// pas un avant/apres a charge egale. Les confondre ferait afficher avec assurance des
/// chiffres sans objet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Nature {
    Tradeoff,
    // Le premier levier de cette nature sera cote GPU. La variante existe des maintenant
    // pour que les phases sachent deja la porter, sans rouvrir leur accumulateur.
    #[expect(dead_code, reason = "aucun profil ne supprime encore de gaspillage")]
    Waste,
}

#[derive(Debug, Clone, Copy)]
pub struct Profile {
    pub id: ProfileId,
    pub targets: Targets,
    pub nature: Nature,
}

const CAPPED: Profile = Profile {
    id: ProfileId::Capped,
    targets: Targets {
        boost_mode: 0,
        throttle_max: 99,
    },
    nature: Nature::Tradeoff,
};

/// Tous les profils, dans l'ordre ou l'interface les presente.
pub const PROFILES: &[Profile] = &[CAPPED];

/// Le profil que l'interrupteur applique tant qu'aucun autre n'est choisi.
pub const DEFAULT: ProfileId = ProfileId::Capped;

/// Une correspondance exhaustive plutot qu'une recherche : un identifiant ajoute sans son
/// profil est une erreur de compilation, pas une panique a l'execution.
pub fn get(id: ProfileId) -> &'static Profile {
    match id {
        ProfileId::Capped => &CAPPED,
    }
}

/// Le profil connu dont les cibles sont exactement celles-ci, s'il y en a un.
pub fn identify(targets: Targets) -> Option<ProfileId> {
    PROFILES.iter().find(|p| p.targets == targets).map(|p| p.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::power::WINDOWS_DEFAULTS;

    #[test]
    fn every_listed_profile_is_the_one_its_id_designates() {
        for p in PROFILES {
            assert_eq!(get(p.id).id, p.id);
            assert_eq!(get(p.id).targets, p.targets);
        }
    }

    /// Deux profils aux memes cibles seraient indiscernables a la relecture : l'etat
    /// affiche dependrait de leur ordre dans la liste.
    #[test]
    fn no_two_profiles_share_their_targets() {
        for (i, a) in PROFILES.iter().enumerate() {
            for b in &PROFILES[i + 1..] {
                assert_ne!(a.targets, b.targets, "{:?} et {:?}", a.id, b.id);
            }
        }
    }

    /// Un profil egal aux defauts de Windows se confondrait avec la machine rendue : on
    /// ne saurait plus dire si l'application intervient.
    #[test]
    fn no_profile_looks_like_an_untouched_machine() {
        for p in PROFILES {
            assert_ne!(p.targets, WINDOWS_DEFAULTS, "{:?}", p.id);
        }
    }

    #[test]
    fn identifies_known_targets_and_only_them() {
        assert_eq!(identify(CAPPED.targets), Some(ProfileId::Capped));
        assert_eq!(identify(WINDOWS_DEFAULTS), None);
        let third_party = Targets {
            boost_mode: 2,
            throttle_max: 80,
        };
        assert_eq!(identify(third_party), None);
    }

    /// Le bridage d'origine ne change pas en devenant un profil.
    #[test]
    fn the_capped_profile_is_the_historic_cap() {
        let t = get(ProfileId::Capped).targets;
        assert_eq!((t.boost_mode, t.throttle_max), (0, 99));
        assert!(t.caps_turbo());
    }
}
