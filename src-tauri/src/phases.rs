//! Accumulation des moyennes par phase de bridage.
//!
//! Cet accumulateur vit cote Rust et non dans l'UI, parce que l'usage vise est
//! precisement celui ou l'UI ne tourne pas : on bascule le bridage en cours de jeu,
//! panneau masque. WebView2 bride les minuteries d'une fenetre cachee — une seconde
//! par tick au debut, puis une minute au-dela de cinq minutes d'occultation. Un
//! accumulateur cote frontend produirait donc des « moyennes » calculees sur quelques
//! echantillons epars, sans que rien ne le signale.
//!
//! Ici l'accumulation suit le thread d'echantillonnage. Elle compte des secondes et non
//! des ticks : la cadence se relache quand le panneau est masque — c'est-a-dire pendant
//! toute la duree qui interesse ce comparatif — et un compteur de ticks annoncerait des
//! durees fausses des le premier changement de cadence.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Serialize, Serializer};

use crate::power::PowerState;
use crate::profiles::{self, Nature, ProfileId, PROFILES};
use crate::sensors::{Metric, Reading};

/// La phase dans laquelle un releve s'accumule.
///
/// Une phase par etat de la machine, et non par position de l'interrupteur : deux
/// profils differents ne se moyennent pas ensemble, et un bridage pose par un outil
/// tiers n'est pas celui d'un de nos profils.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PhaseKey {
    /// Aucun bridage : la reference.
    #[default]
    Free,
    /// Les valeurs exactes d'un profil connu.
    Profile(ProfileId),
    /// Un bridage qu'aucun profil ne decrit.
    Custom,
}

impl PhaseKey {
    /// La phase se lit dans l'etat relu du schema, jamais dans l'intention de
    /// l'utilisateur : le schema peut changer depuis Windows sans passer par nous.
    pub fn of(state: &PowerState) -> Self {
        match state.profile {
            Some(id) => PhaseKey::Profile(id),
            None if state.optimized => PhaseKey::Custom,
            None => PhaseKey::Free,
        }
    }

    /// `None` pour la phase libre : elle est la reference, pas un levier.
    pub fn nature(self) -> Option<Nature> {
        match self {
            PhaseKey::Free => None,
            PhaseKey::Profile(id) => Some(profiles::get(id).nature),
            // Un bridage inconnu retire au moins des performances.
            PhaseKey::Custom => Some(Nature::Tradeoff),
        }
    }
}

/// Une chaine plate — `free`, `custom`, ou l'identifiant du profil — pour que
/// l'interface compare des cles sans demonter une structure.
impl Serialize for PhaseKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            PhaseKey::Free => s.serialize_str("free"),
            PhaseKey::Custom => s.serialize_str("custom"),
            PhaseKey::Profile(id) => id.serialize(s),
        }
    }
}

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

/// Une phase telle que l'interface la recoit.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseEntry {
    pub key: PhaseKey,
    /// Ce que la phase fait payer : c'est ce qui dit comment la comparer.
    pub nature: Option<Nature>,
    #[serde(flatten)]
    pub snapshot: PhaseSnapshot,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhasesSnapshot {
    /// La phase qu'alimentent les releves en ce moment.
    pub current: PhaseKey,
    /// La phase libre et chaque profil connu, toujours, meme vides ; la phase
    /// personnalisee seulement si elle a ete mesuree.
    pub phases: Vec<PhaseEntry>,
}

#[derive(Default)]
struct Phase {
    metrics: BTreeMap<Metric, Acc>,
    seconds: f64,
}

impl Phase {
    fn push(&mut self, reading: &Reading, dt_s: f64) {
        if dt_s.is_finite() && dt_s > 0.0 {
            self.seconds += dt_s;
        }
        for metric in TRACKED {
            if let Some(v) = reading.get(*metric) {
                self.metrics.entry(*metric).or_default().push(v);
            }
        }
    }

    fn snapshot(&self) -> PhaseSnapshot {
        PhaseSnapshot {
            metrics: self
                .metrics
                .iter()
                .filter_map(|(m, a)| a.stat().map(|s| (*m, s)))
                .collect(),
            seconds: self.seconds.round() as u64,
        }
    }
}

#[derive(Default)]
struct Inner {
    current: PhaseKey,
    phases: BTreeMap<PhaseKey, Phase>,
}

/// Un accumulateur par phase, et la cle qui dit lequel alimenter.
#[derive(Default)]
pub struct PhaseRecorder {
    inner: Mutex<Inner>,
}

impl PhaseRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Aiguille les releves suivants vers une phase. Appele au demarrage, a chaque
    /// bascule, et a chaque relecture de l'etat d'alimentation — le schema peut aussi
    /// changer depuis Windows, sans passer par nous.
    pub fn set_phase(&self, key: PhaseKey) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.current = key;
        }
    }

    /// Vrai quand quelque chose bride la machine, quel que soit le profil.
    pub fn is_capped(&self) -> bool {
        self.inner
            .lock()
            .map(|i| i.current != PhaseKey::Free)
            .unwrap_or(false)
    }

    /// `dt_s` est le temps reellement ecoule depuis le releve precedent, tel que mesure
    /// par le thread d'echantillonnage.
    pub fn record(&self, reading: &Reading, dt_s: f64) {
        if let Ok(mut inner) = self.inner.lock() {
            let key = inner.current;
            inner.phases.entry(key).or_default().push(reading, dt_s);
        }
    }

    pub fn snapshot(&self) -> PhasesSnapshot {
        let Ok(inner) = self.inner.lock() else {
            return PhasesSnapshot::default();
        };
        let custom = inner
            .phases
            .contains_key(&PhaseKey::Custom)
            .then_some(PhaseKey::Custom);
        let keys = std::iter::once(PhaseKey::Free)
            .chain(PROFILES.iter().map(|p| PhaseKey::Profile(p.id)))
            .chain(custom);

        PhasesSnapshot {
            current: inner.current,
            phases: keys
                .map(|key| PhaseEntry {
                    key,
                    nature: key.nature(),
                    snapshot: inner
                        .phases
                        .get(&key)
                        .map(Phase::snapshot)
                        .unwrap_or_default(),
                })
                .collect(),
        }
    }

    pub fn reset(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.phases.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPPED: PhaseKey = PhaseKey::Profile(ProfileId::Capped);

    fn reading(temp: f64) -> Reading {
        let mut r = Reading::default();
        r.offer(Metric::CpuTempC, temp, "test");
        r
    }

    fn phase(snap: &PhasesSnapshot, key: PhaseKey) -> &PhaseSnapshot {
        &snap
            .phases
            .iter()
            .find(|e| e.key == key)
            .unwrap_or_else(|| panic!("phase {key:?} absente"))
            .snapshot
    }

    #[test]
    fn routes_samples_to_the_active_phase() {
        let rec = PhaseRecorder::new();
        rec.record(&reading(80.0), 1.0);
        rec.set_phase(CAPPED);
        rec.record(&reading(60.0), 1.0);
        rec.record(&reading(62.0), 1.0);

        let snap = rec.snapshot();
        assert_eq!(phase(&snap, PhaseKey::Free).seconds, 1);
        assert_eq!(phase(&snap, CAPPED).seconds, 2);
        assert_eq!(
            phase(&snap, PhaseKey::Free).metrics[&Metric::CpuTempC].avg,
            80.0
        );
        assert_eq!(phase(&snap, CAPPED).metrics[&Metric::CpuTempC].avg, 61.0);
    }

    #[test]
    fn a_metric_nobody_measures_stays_absent() {
        let rec = PhaseRecorder::new();
        rec.record(&reading(70.0), 1.0);
        let snap = rec.snapshot();
        assert!(!phase(&snap, PhaseKey::Free)
            .metrics
            .contains_key(&Metric::GpuPowerW));
    }

    /// Le comparatif annonce une duree, pas un nombre de mesures. Panneau masque la
    /// cadence se relache, et le meme nombre d'echantillons couvre alors bien plus de
    /// temps : la duree ne peut se lire que dans les intervalles reellement ecoules.
    #[test]
    fn a_changing_cadence_still_yields_a_true_duration() {
        let rec = PhaseRecorder::new();
        rec.record(&reading(70.0), 1.0);
        rec.record(&reading(70.0), 5.0);
        rec.record(&reading(70.0), 5.0);

        let snap = rec.snapshot();
        assert_eq!(phase(&snap, PhaseKey::Free).seconds, 11);
        assert_eq!(phase(&snap, PhaseKey::Free).metrics[&Metric::CpuTempC].n, 3);
    }

    /// Une reprise de veille rend un intervalle enorme : le hub le plafonne, mais
    /// l'accumulateur ne doit pas non plus se laisser abimer par une valeur aberrante.
    #[test]
    fn an_absurd_interval_does_not_corrupt_the_duration() {
        let rec = PhaseRecorder::new();
        rec.record(&reading(70.0), f64::NAN);
        rec.record(&reading(70.0), -3.0);
        rec.record(&reading(70.0), 2.0);

        assert_eq!(phase(&rec.snapshot(), PhaseKey::Free).seconds, 2);
    }

    #[test]
    fn reset_clears_both_phases() {
        let rec = PhaseRecorder::new();
        rec.record(&reading(70.0), 1.0);
        rec.set_phase(CAPPED);
        rec.record(&reading(50.0), 1.0);
        rec.reset();

        let snap = rec.snapshot();
        assert_eq!(phase(&snap, PhaseKey::Free).seconds, 0);
        assert_eq!(phase(&snap, CAPPED).seconds, 0);
        assert!(phase(&snap, CAPPED).metrics.is_empty());
    }

    /// Un bridage tiers et un profil ne se moyennent pas ensemble.
    #[test]
    fn a_third_party_cap_accumulates_apart() {
        let rec = PhaseRecorder::new();
        rec.set_phase(CAPPED);
        rec.record(&reading(60.0), 1.0);
        rec.set_phase(PhaseKey::Custom);
        rec.record(&reading(70.0), 1.0);

        let snap = rec.snapshot();
        assert_eq!(phase(&snap, CAPPED).metrics[&Metric::CpuTempC].avg, 60.0);
        assert_eq!(
            phase(&snap, PhaseKey::Custom).metrics[&Metric::CpuTempC].avg,
            70.0
        );
        assert_eq!(snap.current, PhaseKey::Custom);
    }

    /// Le comparatif garde ses colonnes meme vides ; la phase personnalisee n'apparait
    /// que si elle a existe.
    #[test]
    fn free_and_every_profile_are_always_listed() {
        let snap = PhaseRecorder::new().snapshot();
        let keys: Vec<PhaseKey> = snap.phases.iter().map(|e| e.key).collect();
        assert_eq!(keys.first(), Some(&PhaseKey::Free));
        for p in PROFILES {
            assert!(keys.contains(&PhaseKey::Profile(p.id)));
        }
        assert!(!keys.contains(&PhaseKey::Custom));
    }

    #[test]
    fn the_key_follows_the_state_read_back() {
        use crate::power::{PowerState, Targets};
        let state = |boost_mode, throttle_max| {
            PowerState::new(
                "guid".into(),
                "Balanced".into(),
                Targets {
                    boost_mode,
                    throttle_max,
                    epp: None,
                },
                true,
            )
        };
        assert_eq!(PhaseKey::of(&state(2, 100)), PhaseKey::Free);
        assert_eq!(PhaseKey::of(&state(0, 99)), CAPPED);
        assert_eq!(PhaseKey::of(&state(2, 80)), PhaseKey::Custom);
    }

    #[test]
    fn only_the_reference_has_no_nature() {
        assert_eq!(PhaseKey::Free.nature(), None);
        assert_eq!(CAPPED.nature(), Some(Nature::Tradeoff));
        assert_eq!(PhaseKey::Custom.nature(), Some(Nature::Tradeoff));
    }

    /// Les cles voyagent en chaines plates, et un profil ne doit jamais porter le nom
    /// d'une cle reservee : l'interface les confondrait.
    #[test]
    fn keys_serialize_flat_and_never_collide() {
        let json = |k: PhaseKey| serde_json::to_string(&k).unwrap();
        assert_eq!(json(PhaseKey::Free), "\"free\"");
        assert_eq!(json(PhaseKey::Custom), "\"custom\"");
        assert_eq!(json(CAPPED), "\"capped\"");
        for p in PROFILES {
            let name = json(PhaseKey::Profile(p.id));
            assert_ne!(name, json(PhaseKey::Free));
            assert_ne!(name, json(PhaseKey::Custom));
        }
    }
}
