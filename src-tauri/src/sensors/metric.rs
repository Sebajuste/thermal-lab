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
    GpuClockMhz,
    GpuUtilPct,
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
        Metric::GpuUtilPct,
    ];

    pub const fn unit(self) -> &'static str {
        match self {
            Metric::CpuTempC | Metric::BoardTempC | Metric::GpuTempC => "°C",
            Metric::CpuPowerW | Metric::GpuPowerW => "W",
            Metric::CpuMaxCorePct
            | Metric::CpuAvgPerfPct
            | Metric::CpuUtilPct
            | Metric::GpuUtilPct => "%",
            Metric::CpuNominalMhz | Metric::GpuClockMhz => "MHz",
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

/// Le releve d'un cycle : ce que l'ensemble des fournisseurs a su mesurer.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub values: BTreeMap<Metric, Sample>,
    pub cpu_name: Option<String>,
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
