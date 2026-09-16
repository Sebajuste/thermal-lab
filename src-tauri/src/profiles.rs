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
    /// Turbo interdit, plafond a 80 % du nominal, preference d'energie tiree vers
    /// l'economie. Perd des performances meme hors turbo.
    Aggressive,
}

impl Default for ProfileId {
    fn default() -> Self {
        DEFAULT
    }
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

/// Ne fixe que les deux leviers historiques. La coupure du turbo vaut pour toutes les
/// classes de coeurs ; le reste garde sa valeur d'origine.
const CAPPED: Profile = Profile {
    id: ProfileId::Capped,
    targets: Targets::cpu(0, 99),
    nature: Nature::Tradeoff,
};

/// Les memes valeurs sur les deux classes de coeurs. Sur un processeur hybride, poser
/// seulement la classe 0 ne plafonne que les coeurs E — ceux qu'un jeu sollicite le
/// moins.
///
/// 60 n'est pas une valeur inventee : c'est celle que Windows pose lui-meme dans son
/// schema Economie d'energie. 80 % est un point de depart, a juger au comparatif.
const AGGRESSIVE: Profile = Profile {
    id: ProfileId::Aggressive,
    targets: Targets {
        epp: Some(60),
        throttle_max_1: Some(80),
        epp_1: Some(60),
        ..Targets::cpu(0, 80)
    },
    nature: Nature::Tradeoff,
};

/// Tous les profils, du plus leger au plus appuye : c'est l'ordre de l'interface.
pub const PROFILES: &[Profile] = &[CAPPED, AGGRESSIVE];

/// Ce que l'interface a besoin de savoir d'un profil pour le presenter.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    pub id: ProfileId,
    pub nature: Nature,
    #[serde(flatten)]
    pub targets: Targets,
}

pub fn catalog() -> Vec<ProfileInfo> {
    PROFILES
        .iter()
        .map(|p| ProfileInfo {
            id: p.id,
            nature: p.nature,
            targets: p.targets,
        })
        .collect()
}

/// Le profil que l'interrupteur applique tant qu'aucun autre n'est choisi.
pub const DEFAULT: ProfileId = ProfileId::Capped;

/// Une correspondance exhaustive plutot qu'une recherche : un identifiant ajoute sans son
/// profil est une erreur de compilation, pas une panique a l'execution.
pub fn get(id: ProfileId) -> &'static Profile {
    match id {
        ProfileId::Capped => &CAPPED,
        ProfileId::Aggressive => &AGGRESSIVE,
    }
}

impl Profile {
    /// Des valeurs relues correspondent a ce profil. Un levier que le profil ne fixe pas
    /// ne compte pas : il est revenu a sa valeur d'origine, quelle qu'elle soit.
    fn matches(&self, read: Targets) -> bool {
        let fits = |want: Option<u32>, got: Option<u32>| want.is_none_or(|w| got == Some(w));
        let t = self.targets;
        t.boost_mode == read.boost_mode
            && t.throttle_max == read.throttle_max
            && fits(t.epp, read.epp)
            && fits(t.throttle_max_1, read.throttle_max_1)
            && fits(t.epp_1, read.epp_1)
    }
}

/// Le profil connu que des valeurs relues designent, s'il y en a un.
pub fn identify(read: Targets) -> Option<ProfileId> {
    PROFILES.iter().find(|p| p.matches(read)).map(|p| p.id)
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

    /// Deux profils qu'une meme machine pourrait satisfaire seraient indiscernables a la
    /// relecture : l'etat affiche dependrait de leur ordre dans la liste. Un levier non
    /// fixe accepte toutes les valeurs, donc il ne suffit pas que les cibles different.
    #[test]
    fn no_machine_state_matches_two_profiles() {
        for (i, a) in PROFILES.iter().enumerate() {
            for b in &PROFILES[i + 1..] {
                let compatible = |x: Option<u32>, y: Option<u32>| match (x, y) {
                    (Some(x), Some(y)) => x == y,
                    _ => true,
                };
                let (a_, b_) = (a.targets, b.targets);
                let overlap = a_.boost_mode == b_.boost_mode
                    && a_.throttle_max == b_.throttle_max
                    && compatible(a_.epp, b_.epp)
                    && compatible(a_.throttle_max_1, b_.throttle_max_1)
                    && compatible(a_.epp_1, b_.epp_1);
                assert!(!overlap, "{:?} et {:?} se confondent", a.id, b.id);
            }
        }
    }

    /// L'interrupteur doit s'allumer pour chaque profil : un profil applique qui laisse
    /// l'interrupteur eteint ne pourrait pas etre rendu depuis l'interface.
    #[test]
    fn every_profile_turns_the_switch_on() {
        use crate::power::PowerState;
        for p in PROFILES {
            // Une machine relue a toutes ses valeurs : les leviers non fixes y ont
            // celles d'un schema Equilibre.
            let balanced = Targets {
                epp: Some(33),
                throttle_max_1: Some(100),
                epp_1: Some(33),
                ..Targets::cpu(2, 100)
            };
            let read = p.targets.or(balanced);
            let state = PowerState::new("g".into(), "n".into(), read, true);
            assert!(state.optimized, "{:?}", p.id);
            assert_eq!(state.profile, Some(p.id));
        }
    }

    /// Un profil egal aux defauts de Windows se confondrait avec la machine rendue : on
    /// ne saurait plus dire si l'application intervient.
    #[test]
    fn no_profile_looks_like_an_untouched_machine() {
        for p in PROFILES {
            assert!(!p.matches(WINDOWS_DEFAULTS), "{:?}", p.id);
        }
    }

    #[test]
    fn identifies_known_targets_and_only_them() {
        // (boost, plafond E, EPP E, plafond P, EPP P)
        let read = |b, t, e, t1, e1| Targets {
            boost_mode: b,
            throttle_max: t,
            epp: e,
            throttle_max_1: t1,
            epp_1: e1,
        };
        // Le bridage leger ne fixe que les leviers historiques : le reste est libre.
        let light = read(0, 99, Some(33), Some(100), Some(33));
        assert_eq!(identify(light), Some(ProfileId::Capped));
        assert_eq!(
            identify(read(0, 99, None, None, None)),
            Some(ProfileId::Capped)
        );

        // Le profil agressif exige ses valeurs sur les deux classes.
        let aggressive = read(0, 80, Some(60), Some(80), Some(60));
        assert_eq!(identify(aggressive), Some(ProfileId::Aggressive));
        assert_eq!(identify(read(0, 80, Some(33), Some(80), Some(60))), None);
        assert_eq!(identify(read(0, 80, Some(60), None, Some(60))), None);

        assert_eq!(identify(read(2, 100, Some(33), Some(100), Some(33))), None);
    }

    /// L'etat laisse par la premiere version du profil agressif, qui ne posait que la
    /// classe 0 : coeurs E plafonnes, coeurs P libres. Ce n'est pas le profil agressif,
    /// et le relire comme tel masquerait le defaut.
    #[test]
    fn half_applied_aggressive_is_not_aggressive() {
        let half = Targets {
            epp: Some(60),
            throttle_max_1: Some(100),
            epp_1: Some(33),
            ..Targets::cpu(0, 80)
        };
        assert_eq!(identify(half), None);
        assert!(half.caps_turbo(), "reste un bridage, externe");
    }

    #[test]
    fn aggressive_caps_both_core_classes_alike() {
        let t = get(ProfileId::Aggressive).targets;
        assert_eq!(t.throttle_max_1, Some(t.throttle_max));
        assert_eq!(t.epp_1, t.epp);
    }

    /// Le bridage d'origine ne change pas en devenant un profil.
    #[test]
    fn the_capped_profile_is_the_historic_cap() {
        assert_eq!(get(ProfileId::Capped).targets, Targets::cpu(0, 99));
        assert!(Targets::cpu(0, 99).caps_turbo());
    }
}
