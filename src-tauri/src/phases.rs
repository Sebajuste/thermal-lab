//! Accumulation des moyennes par phase de bridage.
//!
//! Cet accumulateur vit cote Rust et non dans l'UI, parce que l'usage vise est
//! precisement celui ou l'UI ne tourne pas : on bascule le bridage en cours de jeu,
//! panneau masque. WebView2 bride les minuteries d'une fenetre cachee — une seconde
//! par tick au debut, puis une minute au-dela de cinq minutes d'occultation. Un
//! accumulateur cote frontend produirait donc des « moyennes » calculees sur quelques
//! echantillons epars, sans que rien ne le signale.
//!
//! Ici l'accumulation suit le thread d'echantillonnage : un tick mesure, un tick compte.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::Serialize;

use crate::sensors::{Metric, Reading};

/// Les grandeurs retenues dans le comparatif. Les autres sont mesurees et affichees en
/// direct, mais leur moyenne n'apprend rien sur l'effet du bridage.
pub const TRACKED: &[Metric] = &[
    Metric::CpuMaxCorePct,
    Metric::CpuTempC,
    Metric::CpuPowerW,
    Metric::GpuTempC,
    Metric::GpuPowerW,
];

#[derive(Debug, Clone, Copy)]
struct Acc {
    n: u64,
    sum: f64,
    min: f64,
    max: f64,
}

impl Default for Acc {
    fn default() -> Self {
        Self {
            n: 0,
            sum: 0.0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
        }
    }
}

impl Acc {
    fn push(&mut self, v: f64) {
        if !v.is_finite() {
            return;
        }
        self.n += 1;
        self.sum += v;
        self.min = self.min.min(v);
        self.max = self.max.max(v);
    }

    fn stat(&self) -> Option<Stat> {
        (self.n > 0).then(|| Stat {
            avg: self.sum / self.n as f64,
            min: self.min,
            max: self.max,
            n: self.n,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stat {
    pub avg: f64,
    pub min: f64,
    pub max: f64,
    /// Nombre d'echantillons retenus : une moyenne sur trois points ne vaut pas une
    /// moyenne sur trois cents, et l'UI doit pouvoir le dire.
    pub n: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseSnapshot {
    pub metrics: BTreeMap<Metric, Stat>,
    /// Duree passee dans cet etat, en secondes de mesure effectives.
    pub seconds: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhasesSnapshot {
    pub optimized: PhaseSnapshot,
    pub free: PhaseSnapshot,
}

#[derive(Default)]
struct Phase {
    metrics: BTreeMap<Metric, Acc>,
    ticks: u64,
}

impl Phase {
    fn push(&mut self, reading: &Reading) {
        self.ticks += 1;
        for metric in TRACKED {
            if let Some(v) = reading.get(*metric) {
                self.metrics.entry(*metric).or_default().push(v);
            }
        }
    }

    fn snapshot(&self, period_s: f64) -> PhaseSnapshot {
        PhaseSnapshot {
            metrics: self
                .metrics
                .iter()
                .filter_map(|(m, a)| a.stat().map(|s| (*m, s)))
                .collect(),
            seconds: (self.ticks as f64 * period_s).round() as u64,
        }
    }
}

#[derive(Default)]
struct Inner {
    optimized: bool,
    on: Phase,
    off: Phase,
}

/// Deux accumulateurs, un par etat du bridage, et le drapeau qui dit lequel alimenter.
pub struct PhaseRecorder {
    inner: Mutex<Inner>,
    period_s: f64,
}

impl PhaseRecorder {
    pub fn new(period_s: f64) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            period_s,
        }
    }

    /// Aiguille les releves suivants vers l'une ou l'autre phase. Appele au demarrage,
    /// a chaque bascule, et a chaque relecture de l'etat d'alimentation — le schema peut
    /// aussi changer depuis Windows, sans passer par nous.
    pub fn set_optimized(&self, on: bool) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.optimized = on;
        }
    }

    pub fn is_optimized(&self) -> bool {
        self.inner.lock().map(|i| i.optimized).unwrap_or(false)
    }

    pub fn record(&self, reading: &Reading) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.optimized {
                inner.on.push(reading);
            } else {
                inner.off.push(reading);
            }
        }
    }

    pub fn snapshot(&self) -> PhasesSnapshot {
        match self.inner.lock() {
            Ok(inner) => PhasesSnapshot {
                optimized: inner.on.snapshot(self.period_s),
                free: inner.off.snapshot(self.period_s),
            },
            Err(_) => PhasesSnapshot::default(),
        }
    }

    pub fn reset(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.on = Phase::default();
            inner.off = Phase::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(temp: f64) -> Reading {
        let mut r = Reading::default();
        r.offer(Metric::CpuTempC, temp, "test");
        r
    }

    #[test]
    fn routes_samples_to_the_active_phase() {
        let rec = PhaseRecorder::new(1.0);
        rec.record(&reading(80.0));
        rec.set_optimized(true);
        rec.record(&reading(60.0));
        rec.record(&reading(62.0));

        let snap = rec.snapshot();
        assert_eq!(snap.free.seconds, 1);
        assert_eq!(snap.optimized.seconds, 2);
        assert_eq!(snap.free.metrics[&Metric::CpuTempC].avg, 80.0);
        assert_eq!(snap.optimized.metrics[&Metric::CpuTempC].avg, 61.0);
    }

    #[test]
    fn a_metric_nobody_measures_stays_absent() {
        let rec = PhaseRecorder::new(1.0);
        rec.record(&reading(70.0));
        let snap = rec.snapshot();
        assert!(!snap.free.metrics.contains_key(&Metric::GpuPowerW));
    }

    #[test]
    fn counts_seconds_from_the_sampling_period() {
        let rec = PhaseRecorder::new(2.0);
        rec.record(&reading(70.0));
        rec.record(&reading(70.0));
        assert_eq!(rec.snapshot().free.seconds, 4);
    }

    #[test]
    fn reset_clears_both_phases() {
        let rec = PhaseRecorder::new(1.0);
        rec.record(&reading(70.0));
        rec.set_optimized(true);
        rec.record(&reading(50.0));
        rec.reset();

        let snap = rec.snapshot();
        assert_eq!(snap.free.seconds, 0);
        assert_eq!(snap.optimized.seconds, 0);
        assert!(snap.optimized.metrics.is_empty());
    }
}
