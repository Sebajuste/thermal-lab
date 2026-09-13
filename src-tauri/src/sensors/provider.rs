//! Le contrat que respecte toute source de mesure.
//!
//! Un fournisseur declare ce qu'il sait mesurer, tente de s'etablir, puis alimente le
//! releve. Il n'a aucune connaissance des autres : l'arbitrage appartient au registre
//! (ordre de priorite) et a `Reading::offer` (premier servi gagne).

use serde::Serialize;

use super::metric::{Metric, Reading};
use super::wmi_context::WmiContext;

/// Ce dont depend un fournisseur pour fonctionner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    /// Ne depend que de Windows : disponible sur toute machine, sans privilege.
    Builtin,
    /// Depend d'un outil tiers installe et lance, qui apporte son propre pilote noyau.
    External,
}

/// Resultat d'une tentative d'etablissement.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "state", content = "detail")]
pub enum ProbeState {
    /// Operationnel.
    Ready,
    /// Absent pour une raison attendue — ce n'est pas une erreur, et `hint` dit quoi
    /// faire pour y remedier.
    Unavailable {
        reason: String,
        hint: Option<String>,
    },
    /// Present mais cassé : la source a repondu autre chose que ce qui etait attendu.
    Failed { error: String },
}

impl ProbeState {
    pub fn is_ready(&self) -> bool {
        matches!(self, ProbeState::Ready)
    }

    pub fn unavailable(reason: impl Into<String>, hint: impl Into<String>) -> Self {
        ProbeState::Unavailable {
            reason: reason.into(),
            hint: Some(hint.into()),
        }
    }

    /// Absent sans rien a faire pour y remedier : la raison se suffit.
    pub fn unavailable_only(reason: impl Into<String>) -> Self {
        ProbeState::Unavailable {
            reason: reason.into(),
            hint: None,
        }
    }
}

/// Carte d'identite d'un fournisseur, telle qu'affichee a l'utilisateur.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    /// Identifiant stable, utilise comme provenance dans `Sample`.
    pub id: &'static str,
    pub name: &'static str,
    pub kind: ProviderKind,
    /// Les grandeurs que ce fournisseur revendique. Sert a expliquer a l'utilisateur ce
    /// qu'il gagnerait a installer un outil donne.
    pub provides: &'static [Metric],
    /// Ou se procurer l'outil, pour les fournisseurs externes.
    pub url: Option<&'static str>,
}

/// Ce qu'un cycle de mesure apprend sur la source elle-meme, independamment des valeurs
/// qu'elle a pu poser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sampled {
    /// La source a repondu — meme si elle n'a rien apporte au releve, parce qu'une
    /// source plus fiable avait deja pose ses grandeurs.
    Answered,
    /// La source ne repond plus : l'outil a ete ferme, sa section a disparu. Un
    /// fournisseur etabli une fois ne l'est pas pour toujours, et le hub doit
    /// l'apprendre autrement qu'en publiant du silence.
    Lost,
}

/// Ce que le hub met a disposition au moment d'etablir une source.
pub struct ProbeContext<'a> {
    /// Absent si l'initialisation COM a echoue : les fournisseurs WMI doivent alors
    /// se declarer indisponibles plutot que de tenter leur chance.
    pub wmi: Option<&'a WmiContext>,
}

/// Les fournisseurs sont construits et utilises sur le seul thread d'echantillonnage :
/// pas de borne `Send`, ce qui laisse un pilote detenir un handle COM sans contorsion.
pub trait Provider {
    fn info(&self) -> ProviderInfo;

    /// Tente d'etablir l'acces. Le hub rappelle cette methode periodiquement tant que
    /// l'etat n'est pas `Ready`, ce qui permet de brancher un outil a chaud.
    fn probe(&mut self, ctx: &ProbeContext<'_>) -> ProbeState;

    /// Alimente le releve. N'ecrase jamais une valeur deja posee : `Reading::offer`
    /// s'en charge, le fournisseur n'a pas a s'en soucier.
    ///
    /// Le retour ne porte que sur la source : `Lost` des qu'elle ne repond plus, jamais
    /// parce qu'une grandeur manque. Une carte sans capteur de puissance repond quand
    /// meme.
    fn sample(&mut self, out: &mut Reading) -> Sampled;
}
