//! Boucle d'echantillonnage et etat des fournisseurs.
//!
//! Un thread dedie detient COM, les connexions WMI et les fournisseurs — rien de tout
//! cela ne traverse les threads. Les commandes Tauri se contentent de lire le dernier
//! releve publie.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::metric::Reading;
use super::provider::{ProbeContext, ProbeState, Provider, ProviderInfo, Sampled};
use super::registry;
use super::wmi_context::WmiContext;

/// Nombre de cycles entre deux tentatives sur un fournisseur non etabli. Permet de
/// brancher un outil a chaud sans redemarrer l'application.
const REPROBE_EVERY: u32 = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    #[serde(flatten)]
    pub info: ProviderInfo,
    #[serde(flatten)]
    pub state: ProbeState,
}

pub struct SensorHub {
    latest: Arc<Mutex<Reading>>,
    statuses: Arc<Mutex<Vec<ProviderStatus>>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl SensorHub {
    /// `on_reading` est appele sur le thread d'echantillonnage, a chaque cycle, avec le
    /// releve qui vient d'etre publie. C'est le seul point d'accroche du pipeline vers
    /// l'exterieur : il permet d'accumuler ou de notifier au rythme de la mesure, sans
    /// dependre de la presence d'une interface.
    pub fn start<F>(period: Duration, on_reading: F) -> Self
    where
        F: Fn(&Reading) + Send + 'static,
    {
        let latest = Arc::new(Mutex::new(Reading::default()));
        let statuses = Arc::new(Mutex::new(Vec::new()));

        let reading_sink = Arc::clone(&latest);
        let status_sink = Arc::clone(&statuses);

        thread::spawn(move || {
            // L'echec de COM n'est pas fatal : les fournisseurs qui n'en dependent pas
            // (memoire partagee, nvidia-smi) restent operationnels.
            let wmi = WmiContext::new().ok();
            let ctx = ProbeContext { wmi: wmi.as_ref() };

            let mut providers = registry::build();
            let mut states: Vec<ProbeState> = providers.iter_mut().map(|p| p.probe(&ctx)).collect();
            publish_statuses(&status_sink, &providers, &states);

            let mut tick: u32 = 0;
            loop {
                tick = tick.wrapping_add(1);
                let retry = tick.is_multiple_of(REPROBE_EVERY);
                let mut changed = false;

                let mut reading = Reading::default();
                for (provider, state) in providers.iter_mut().zip(states.iter_mut()) {
                    changed |= step(provider.as_mut(), state, &ctx, retry, &mut reading);
                }
                reading.ts_ms = now_ms();

                on_reading(&reading);
                if let Ok(mut slot) = reading_sink.lock() {
                    *slot = reading;
                }
                if changed {
                    publish_statuses(&status_sink, &providers, &states);
                }

                thread::sleep(period);
            }
        });

        Self { latest, statuses }
    }

    pub fn latest(&self) -> Reading {
        self.latest.lock().map(|r| r.clone()).unwrap_or_default()
    }

    pub fn statuses(&self) -> Vec<ProviderStatus> {
        self.statuses.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// Un cycle pour une source : la retenter si elle n'est pas etablie, la lire si elle
/// l'est, et la redemander des qu'elle cesse de repondre. Renvoie vrai quand l'etat
/// affiche a change.
///
/// Ce dernier cas manquait : un fournisseur etabli ne l'etait plus jamais que sur le
/// papier. Fermer LibreHardwareMonitor laissait `actif` a l'ecran pendant que le releve
/// se vidait — l'inverse exact du symptome que le modele de capacites doit eviter.
fn step(
    provider: &mut dyn Provider,
    state: &mut ProbeState,
    ctx: &ProbeContext<'_>,
    retry: bool,
    reading: &mut Reading,
) -> bool {
    let mut changed = false;

    if !state.is_ready() {
        if !retry {
            return false;
        }
        *state = provider.probe(ctx);
        changed = state.is_ready();
        if !changed {
            return false;
        }
    }

    if provider.sample(reading) == Sampled::Lost {
        // Perdue en cours de cycle : la redemander tout de suite donne la raison exacte,
        // et rattrape l'incident passager — une lecture ratee sous charge, par exemple.
        let fresh = provider.probe(ctx);
        changed |= !fresh.is_ready();
        *state = fresh;
    }

    changed
}

fn publish_statuses(
    sink: &Arc<Mutex<Vec<ProviderStatus>>>,
    providers: &[Box<dyn super::provider::Provider>],
    states: &[ProbeState],
) {
    let snapshot: Vec<ProviderStatus> = providers
        .iter()
        .zip(states.iter())
        .map(|(p, s)| ProviderStatus {
            info: p.info(),
            state: s.clone(),
        })
        .collect();

    if let Ok(mut slot) = sink.lock() {
        *slot = snapshot;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::metric::Metric;
    use crate::sensors::provider::ProviderKind;

    /// Une source dont on regle a l'avance ce qu'elle repondra, et qui compte ce qu'on
    /// lui demande : c'est l'enchainement `probe`/`sample` qui est teste, pas un pilote.
    struct Stub {
        probe_state: ProbeState,
        outcome: Sampled,
        probes: u32,
        samples: u32,
    }

    impl Stub {
        fn new(probe_state: ProbeState, outcome: Sampled) -> Self {
            Self {
                probe_state,
                outcome,
                probes: 0,
                samples: 0,
            }
        }
    }

    impl Provider for Stub {
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                id: "stub",
                name: "Stub",
                kind: ProviderKind::Builtin,
                provides: &[Metric::CpuTempC],
                url: None,
            }
        }

        fn probe(&mut self, _ctx: &ProbeContext<'_>) -> ProbeState {
            self.probes += 1;
            self.probe_state.clone()
        }

        fn sample(&mut self, out: &mut Reading) -> Sampled {
            self.samples += 1;
            if self.outcome == Sampled::Answered {
                out.offer(Metric::CpuTempC, 42.0, "stub");
            }
            self.outcome
        }
    }

    fn run(stub: &mut Stub, state: &mut ProbeState, retry: bool) -> (bool, Reading) {
        let mut reading = Reading::default();
        let ctx = ProbeContext { wmi: None };
        let changed = step(stub, state, &ctx, retry, &mut reading);
        (changed, reading)
    }

    /// Le defaut que ce decoupage corrige : un outil ferme laissait `actif` a l'ecran,
    /// puisqu'une source etablie n'etait plus jamais reinterrogee.
    #[test]
    fn a_source_that_stops_answering_loses_its_ready_state() {
        let mut stub = Stub::new(ProbeState::unavailable_only("outil ferme"), Sampled::Lost);
        let mut state = ProbeState::Ready;

        let (changed, _) = run(&mut stub, &mut state, false);

        assert!(changed, "l'interface doit apprendre la perte");
        assert!(!state.is_ready());
    }

    /// Une lecture ratee n'est pas une disparition : si la source repond encore au
    /// `probe` qui suit, rien ne doit bouger a l'ecran.
    #[test]
    fn a_transient_failure_does_not_demote_a_live_source() {
        let mut stub = Stub::new(ProbeState::Ready, Sampled::Lost);
        let mut state = ProbeState::Ready;

        let (changed, _) = run(&mut stub, &mut state, false);

        assert!(!changed);
        assert!(state.is_ready());
    }

    /// Une source non etablie n'est retentee qu'un cycle sur cinq : c'est ce qui empeche
    /// un outil absent de couter une tentative a chaque mesure.
    #[test]
    fn an_unestablished_source_waits_for_the_retry_tick() {
        let mut stub = Stub::new(ProbeState::Ready, Sampled::Answered);
        let mut state = ProbeState::unavailable_only("absent");

        let (changed, _) = run(&mut stub, &mut state, false);

        assert!(!changed);
        assert_eq!((stub.probes, stub.samples), (0, 0));
    }

    /// Branche a chaud, la source est lue dans le cycle meme ou elle apparait : attendre
    /// le suivant afficherait `actif` sans valeur.
    #[test]
    fn a_source_that_appears_is_read_at_once() {
        let mut stub = Stub::new(ProbeState::Ready, Sampled::Answered);
        let mut state = ProbeState::unavailable_only("absent");

        let (changed, reading) = run(&mut stub, &mut state, true);

        assert!(changed);
        assert!(state.is_ready());
        assert_eq!(reading.get(Metric::CpuTempC), Some(42.0));
    }
}
