//! Vocabulaire commun a tous les fournisseurs : ce qui peut etre mesure, et le releve
//! qui agrege leurs contributions.
//!
//! Aucun fournisseur n'apparait ici. C'est la condition pour que le pipeline reste
//! agnostique : ajouter une source ne touche pas ce fichier, sauf a mesurer une grandeur
//! reellement nouvelle.

use serde::Serialize;
use std::collections::BTreeMap;

/// Une grandeur mesurable, independamment de qui sait la fournir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Metric {
    /// Temperature de package CPU : maximum sur les coeurs.
    CpuTempC,
    /// Puissance consommee par le package CPU.
    CpuPowerW,
    /// Frequence du coeur le plus rapide, en % du nominal. Au-dela de 100, turbo engage.
    CpuMaxCorePct,
    /// Moyenne sur tous les processeurs logiques, en % du nominal.
    CpuAvgPerfPct,
    CpuUtilPct,
    CpuNominalMhz,
    /// Zone thermique de la carte mere. Indicatif : ce n'est pas le die.
    BoardTempC,
    GpuTempC,
    GpuPowerW,
    /// Frequence du domaine SM, l'analogue GPU de la frequence coeur.
    GpuClockMhz,
    /// Plafond de frequence SM annonce par la carte. Constante propre au modele, sans
    /// laquelle `GpuClockMhz` ne se compare a rien : 2115 MHz est un repos sur l'une et
    /// un plein regime sur l'autre.
    GpuClockMaxMhz,
    GpuUtilPct,
    /// Etat de performance annonce par le pilote, en indice : 0 le plus performant, 8 ou
    /// 12 le repos selon les cartes. Contrairement a la frequence, il est normalise par
    /// le pilote carte par carte — c'est lui qui dit dans quel etat il se tient, sans
    /// dependre d'un plafond a interpreter.
    GpuPerfStateIndex,
    /// Un ecran est-il initialise sur cette carte — 1 ou 0. C'est ce qui separe une carte
    /// eveillee pour rien d'une carte qui fait son travail : sur une tour, ou sur un
    /// portable dont le MUX est en mode discret, elle balaie une dalle et son eveil est
    /// legitime.
    GpuDisplayActive,
    /// Le pilote declare-t-il lui-meme n'avoir rien a faire executer — 1 ou 0, depuis le
    /// drapeau `GpuIdle` des raisons d'evenement d'horloge. Plus direct qu'une frequence
    /// a interpreter : c'est l'affirmation du pilote, pas une deduction.
    GpuDriverIdle,
    /// Nombre de process qui tiennent de la memoire dediee sur la carte NVIDIA. Un seul
    /// suffit a interdire son extinction, meme a charge nulle.
    GpuHolderCount,
    /// Occupation du decodeur video (NVDEC). Distincte de `GpuUtilPct`, qui reste bas
    /// pendant une lecture video : c'est ce qui separe un GPU inoccupe d'un GPU qui
    /// decode.
    GpuDecodeUtilPct,
    /// Occupation de l'encodeur video (NVENC) : capture, diffusion.
    GpuEncodeUtilPct,
}

impl Metric {
    /// Toutes les grandeurs connues du pipeline, pour distinguer ce qui est mesure de
    /// ce qui manque.
    pub const ALL: &'static [Metric] = &[
        Metric::CpuTempC,
        Metric::CpuPowerW,
        Metric::CpuMaxCorePct,
        Metric::CpuAvgPerfPct,
        Metric::CpuUtilPct,
        Metric::CpuNominalMhz,
        Metric::BoardTempC,
        Metric::GpuTempC,
        Metric::GpuPowerW,
        Metric::GpuClockMhz,
        Metric::GpuClockMaxMhz,
        Metric::GpuUtilPct,
        Metric::GpuPerfStateIndex,
        Metric::GpuDisplayActive,
        Metric::GpuDriverIdle,
        Metric::GpuHolderCount,
        Metric::GpuDecodeUtilPct,
        Metric::GpuEncodeUtilPct,
    ];

    pub const fn unit(self) -> &'static str {
        match self {
            Metric::CpuTempC | Metric::BoardTempC | Metric::GpuTempC => "°C",
            Metric::CpuPowerW | Metric::GpuPowerW => "W",
            Metric::CpuMaxCorePct
            | Metric::CpuAvgPerfPct
            | Metric::CpuUtilPct
            | Metric::GpuUtilPct
            | Metric::GpuDecodeUtilPct
            | Metric::GpuEncodeUtilPct => "%",
            Metric::CpuNominalMhz | Metric::GpuClockMhz | Metric::GpuClockMaxMhz => "MHz",
            // Un indice d'etat, deux booleens et un compte : aucune grandeur physique.
            Metric::GpuPerfStateIndex
            | Metric::GpuDisplayActive
            | Metric::GpuDriverIdle
            | Metric::GpuHolderCount => "",
        }
    }
}

/// Une valeur, et le fournisseur qui l'a produite. La provenance remonte jusqu'a l'UI :
/// l'utilisateur doit pouvoir savoir d'ou sort un chiffre.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub value: f64,
    pub provider: &'static str,
}

/// Un process qui tient de la memoire sur la carte graphique.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuHolder {
    pub pid: u32,
    /// `None` pour un process qui ne se laisse pas ouvrir : protege, ou deja termine.
    pub name: Option<String>,
    /// `None` quand le compteur de Windows rapporte une valeur impossible : le process
    /// tient bien la carte, mais on ne sait pas combien.
    pub dedicated_mb: Option<f64>,
}

/// Le releve d'un cycle : ce que l'ensemble des fournisseurs a su mesurer.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub values: BTreeMap<Metric, Sample>,
    pub cpu_name: Option<String>,
    /// Les plus gros clients de la carte NVIDIA, du plus gros au plus petit. Le compte
    /// complet est dans `Metric::GpuHolderCount`.
    pub gpu_holders: Vec<GpuHolder>,
    pub ts_ms: u64,
}

impl Reading {
    /// Propose une valeur : **le premier fournisseur servi gagne**.
    ///
    /// Le registre est parcouru par priorite decroissante, donc une source moins fiable
    /// ne peut jamais ecraser une source plus fiable deja passee. C'est toute la regle
    /// de resolution des conflits du pipeline, et elle tient en une ligne.
    pub fn offer(&mut self, metric: Metric, value: f64, provider: &'static str) {
        if !value.is_finite() {
            return;
        }
        self.values
            .entry(metric)
            .or_insert(Sample { value, provider });
    }

    pub fn get(&self, metric: Metric) -> Option<f64> {
        self.values.get(&metric).map(|s| s.value)
    }

    pub fn has(&self, metric: Metric) -> bool {
        self.values.contains_key(&metric)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_provider_wins() {
        let mut r = Reading::default();
        r.offer(Metric::CpuTempC, 61.5, "core-temp");
        r.offer(Metric::CpuTempC, 48.0, "acpi");
        assert_eq!(r.get(Metric::CpuTempC), Some(61.5));
        assert_eq!(r.values[&Metric::CpuTempC].provider, "core-temp");
    }

    #[test]
    fn rejects_non_finite() {
        let mut r = Reading::default();
        r.offer(Metric::CpuTempC, f64::NAN, "x");
        r.offer(Metric::CpuPowerW, f64::INFINITY, "x");
        assert!(!r.has(Metric::CpuTempC));
        assert!(!r.has(Metric::CpuPowerW));
    }

    #[test]
    fn a_failed_provider_leaves_room_for_the_next() {
        let mut r = Reading::default();
        r.offer(Metric::CpuTempC, f64::NAN, "core-temp");
        r.offer(Metric::CpuTempC, 55.0, "libre-hw");
        assert_eq!(r.get(Metric::CpuTempC), Some(55.0));
    }
}
